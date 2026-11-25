//! Bounded Session Pool: Fixed-capacity, thread-safe pool of neural inference sessions
//! with RAII checkout/checkin semantics (ADR 0015).

use anyhow::Result;
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

/// RAII checkout guard for a pooled inference session.
/// Automatically checks the session back into the pool when dropped.
pub struct SessionGuard<'a, T> {
    item: Option<T>,
    sender: &'a SyncSender<T>,
}

impl<'a, T> Deref for SessionGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.item
            .as_ref()
            .expect("SessionGuard item unexpectedly empty")
    }
}

impl<'a, T> DerefMut for SessionGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.item
            .as_mut()
            .expect("SessionGuard item unexpectedly empty")
    }
}

impl<'a, T> Drop for SessionGuard<'a, T> {
    fn drop(&mut self) {
        if let Some(item) = self.item.take() {
            let _ = self.sender.send(item);
        }
    }
}

/// A thread-safe, bounded pool of neural sessions managed via channel checkout/checkin semantics.
pub struct SessionPool<T> {
    sender: SyncSender<T>,
    receiver: Mutex<Receiver<T>>,
    capacity: usize,
}

impl<T> SessionPool<T> {
    /// Creates a new Bounded Session Pool from pre-initialized sessions.
    ///
    /// # Panics
    /// Panics if `sessions` is empty.
    pub fn new(sessions: Vec<T>) -> Self {
        assert!(!sessions.is_empty(), "SessionPool cannot be empty");
        let capacity = sessions.len();
        let (sender, receiver) = sync_channel(capacity);
        for s in sessions {
            sender
                .send(s)
                .expect("Failed to initialize session pool channel");
        }
        Self {
            sender,
            receiver: Mutex::new(receiver),
            capacity,
        }
    }

    /// Checks out a session from the pool. Blocks if all sessions are currently checked out.
    pub fn acquire(&self) -> Result<SessionGuard<'_, T>> {
        let rx = self
            .receiver
            .lock()
            .map_err(|e| anyhow::anyhow!("Session pool receiver lock poisoned: {e}"))?;
        let item = rx
            .recv()
            .map_err(|e| anyhow::anyhow!("Session pool channel closed: {e}"))?;
        Ok(SessionGuard {
            item: Some(item),
            sender: &self.sender,
        })
    }

    /// Returns the total configured capacity of the pool.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_pool_checkout_and_return() {
        let pool = SessionPool::new(vec![10, 20]);
        assert_eq!(pool.capacity(), 2);

        {
            let mut g1 = pool.acquire().unwrap();
            assert_eq!(*g1, 10);
            *g1 = 15;

            let g2 = pool.acquire().unwrap();
            assert_eq!(*g2, 20);
        }

        // Both guards dropped, values returned to pool (FIFO channel order)
        let g3 = pool.acquire().unwrap();
        let g4 = pool.acquire().unwrap();
        let mut vals = vec![*g3, *g4];
        vals.sort();
        assert_eq!(vals, vec![15, 20]);
    }
}
