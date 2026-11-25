//! RAII RunController with deterministic lifecycle, cooperative cancellation, and panic isolation (Phase 12).

use serde::{Deserialize, Serialize};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// High-level lifecycle state for a batch conversion or watcher run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    #[default]
    Idle,
    Running,
    Cancelling,
}

/// Error returned when attempting to start a run while another is active.
#[derive(Debug, Error)]
#[error("conversion run already in progress (run_id: {active_run_id})")]
pub struct RunConflictError {
    pub active_run_id: String,
}

/// Thread-safe controller managing job identities, state transitions, and cooperative cancellation tokens.
#[derive(Clone, Default)]
pub struct RunController {
    state: Arc<Mutex<RunState>>,
    active_run_id: Arc<Mutex<Option<String>>>,
    cancel_token: Arc<AtomicBool>,
}

impl RunController {
    /// Creates a new idle run controller.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RunState::Idle)),
            active_run_id: Arc::new(Mutex::new(None)),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Attempts to transition the controller from `Idle` to `Running`.
    ///
    /// Returns a RAII [`RunGuard`] that resets the controller back to `Idle` when dropped.
    pub fn start_run(
        &self,
        requested_run_id: Option<String>,
    ) -> Result<RunGuard, RunConflictError> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if *state != RunState::Idle {
            let active = self
                .active_run_id
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            return Err(RunConflictError {
                active_run_id: active,
            });
        }

        let run_id = requested_run_id.unwrap_or_else(|| {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            format!("run_{timestamp}")
        });

        *state = RunState::Running;
        *self.active_run_id.lock().unwrap_or_else(|p| p.into_inner()) = Some(run_id.clone());
        self.cancel_token.store(false, Ordering::SeqCst);

        Ok(RunGuard {
            controller: self.clone(),
            run_id,
        })
    }

    /// Requests cooperative cancellation of the active run.
    ///
    /// Transitions state from `Running` to `Cancelling` and asserts the cancellation token.
    pub fn request_cancel(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if *state == RunState::Running {
            *state = RunState::Cancelling;
            self.cancel_token.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// Checks if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.load(Ordering::SeqCst)
    }

    /// Returns the cancellation token atomic reference for passing to worker threads.
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel_token.clone()
    }

    /// Returns the current run state and active run identifier if running.
    pub fn current_state(&self) -> (RunState, Option<String>) {
        let state = *self.state.lock().unwrap_or_else(|p| p.into_inner());
        let run_id = self
            .active_run_id
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        (state, run_id)
    }

    /// Internal cleanup invoked exclusively by [`RunGuard::drop`].
    fn end_run(&self, expected_run_id: &str) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut active = self.active_run_id.lock().unwrap_or_else(|p| p.into_inner());

        if active.as_deref() == Some(expected_run_id) {
            *state = RunState::Idle;
            *active = None;
            self.cancel_token.store(false, Ordering::SeqCst);
        }
    }
}

/// RAII scope guard representing an active conversion run.
///
/// Ensures the associated [`RunController`] deterministically transitions back to `Idle`
/// even if a panic occurs or the task returns early.
pub struct RunGuard {
    controller: RunController,
    run_id: String,
}

impl RunGuard {
    /// Returns the unique identifier for this run.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Returns a reference to the underlying controller.
    pub fn controller(&self) -> &RunController {
        &self.controller
    }

    /// Checks if cancellation has been requested during this run.
    pub fn is_cancelled(&self) -> bool {
        self.controller.is_cancelled()
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.controller.end_run(&self.run_id);
    }
}

/// Executes a per-document conversion closure with strict panic isolation.
///
/// If a panic occurs inside `f`, it is caught, the panic payload formatted into an error message,
/// and returned as `Err(anyhow::Error)` instead of aborting the host process.
pub fn execute_isolated_document<T, F>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(res) => res,
        Err(panic_payload) => {
            let message = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown thread panic during document conversion".to_string()
            };
            Err(anyhow::anyhow!("Panic isolated: {message}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_controller_lifecycle() {
        let controller = RunController::new();
        let (state, run_id) = controller.current_state();
        assert_eq!(state, RunState::Idle);
        assert!(run_id.is_none());

        {
            let guard = controller
                .start_run(Some("test_run_1".to_string()))
                .expect("start run");
            assert_eq!(guard.run_id(), "test_run_1");

            let (state, run_id) = controller.current_state();
            assert_eq!(state, RunState::Running);
            assert_eq!(run_id.as_deref(), Some("test_run_1"));

            // Attempting to start another run must fail with conflict
            assert!(
                controller
                    .start_run(Some("test_run_2".to_string()))
                    .is_err()
            );

            assert!(!controller.is_cancelled());
            assert!(controller.request_cancel());
            assert!(controller.is_cancelled());
            assert_eq!(controller.current_state().0, RunState::Cancelling);
        }

        // After guard drops, controller resets to Idle
        let (state, run_id) = controller.current_state();
        assert_eq!(state, RunState::Idle);
        assert!(run_id.is_none());
        assert!(!controller.is_cancelled());
    }

    #[test]
    fn test_panic_isolation_catches_unwind() {
        let result = execute_isolated_document::<(), _>(|| {
            panic!("fatal parsing assertion failure");
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Panic isolated"));
        assert!(err.contains("fatal parsing assertion failure"));
    }
}
