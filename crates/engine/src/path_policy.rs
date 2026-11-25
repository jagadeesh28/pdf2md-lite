//! Path containment and validation policy for pdfmd-lite.
//!
//! Enforces canonicalization against granted roots, prohibiting path traversal,
//! UNC paths, Windows reserved device names, and trailing dots/spaces.

use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathPolicyError {
    #[error("Path traversal (..) is forbidden: {0}")]
    TraversalDetected(String),
    #[error("UNC network paths are forbidden: {0}")]
    UncPathForbidden(String),
    #[error("Windows reserved device name is forbidden: {0}")]
    ReservedDeviceName(String),
    #[error("Trailing dot or space in component is forbidden: {0}")]
    TrailingDotOrSpace(String),
    #[error("Path {0} is outside of granted roots")]
    OutsideRoots(PathBuf),
    #[error("Failed to canonicalize path {0}: {1}")]
    CanonicalizationError(PathBuf, String),
    #[error("No granted roots are configured in PathPolicy")]
    NoGrantedRoots,
    #[error("Path is empty or invalid")]
    InvalidPath,
}

/// Enforces filesystem boundaries and prevents arbitrary file reads/writes.
#[derive(Debug, Clone)]
pub struct PathPolicy {
    roots: Vec<PathBuf>,
}

impl Default for PathPolicy {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl PathPolicy {
    /// Creates a new PathPolicy with the given granted root directories.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        let mut canonical_roots = Vec::new();
        for r in roots {
            let canonical = dunce::canonicalize(&r).unwrap_or_else(|_| normalize_components(&r));
            if !canonical_roots.contains(&canonical) {
                canonical_roots.push(canonical);
            }
        }
        Self {
            roots: canonical_roots,
        }
    }

    /// Adds a granted root directory to the policy.
    pub fn add_root(&mut self, root: &Path) {
        let canonical = dunce::canonicalize(root).unwrap_or_else(|_| normalize_components(root));
        if !self.roots.contains(&canonical) {
            self.roots.push(canonical);
        }
    }

    /// Returns the currently active granted root directories.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Validates the path syntax for forbidden patterns (traversal, UNC, device names, trailing dots).
    pub fn check_syntax(path: &Path) -> Result<(), PathPolicyError> {
        let path_str = path.to_string_lossy();
        if path_str.is_empty() {
            return Err(PathPolicyError::InvalidPath);
        }

        // UNC checks
        if path_str.starts_with(r"\\") || path_str.starts_with("//") {
            return Err(PathPolicyError::UncPathForbidden(path_str.to_string()));
        }

        for comp in path.components() {
            match comp {
                Component::ParentDir => {
                    return Err(PathPolicyError::TraversalDetected(path_str.to_string()));
                }
                Component::Normal(os_str) => {
                    let s = os_str.to_string_lossy();
                    if s.ends_with('.') || s.ends_with(' ') {
                        return Err(PathPolicyError::TrailingDotOrSpace(s.to_string()));
                    }
                    if is_reserved_device_name(&s) {
                        return Err(PathPolicyError::ReservedDeviceName(s.to_string()));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Checks and canonicalizes an existing path for reading within granted roots.
    pub fn check_read(&self, path: &Path) -> Result<PathBuf, PathPolicyError> {
        Self::check_syntax(path)?;

        if self.roots.is_empty() {
            return Err(PathPolicyError::NoGrantedRoots);
        }

        let canonical = dunce::canonicalize(path).map_err(|e| {
            PathPolicyError::CanonicalizationError(path.to_path_buf(), e.to_string())
        })?;

        if self.is_under_any_root(&canonical) {
            Ok(canonical)
        } else {
            Err(PathPolicyError::OutsideRoots(canonical))
        }
    }

    /// Checks and canonicalizes a path for writing within granted roots.
    /// If the target file does not yet exist, verifies the closest existing parent directory.
    pub fn check_write(&self, path: &Path) -> Result<PathBuf, PathPolicyError> {
        Self::check_syntax(path)?;

        if self.roots.is_empty() {
            return Err(PathPolicyError::NoGrantedRoots);
        }

        let canonical = if path.exists() {
            dunce::canonicalize(path).map_err(|e| {
                PathPolicyError::CanonicalizationError(path.to_path_buf(), e.to_string())
            })?
        } else {
            // Find closest existing ancestor directory
            let mut ancestors = path.ancestors();
            let _ = ancestors.next(); // skip self
            let mut existing_ancestor = None;
            let mut remaining_parts = Vec::new();

            let mut curr = path.parent();
            while let Some(p) = curr {
                if p.exists() {
                    existing_ancestor = Some(p);
                    break;
                }
                if let Some(file_name) = p.file_name() {
                    remaining_parts.push(file_name);
                }
                curr = p.parent();
            }

            if let Some(ancestor) = existing_ancestor {
                let canonical_ancestor = dunce::canonicalize(ancestor).map_err(|e| {
                    PathPolicyError::CanonicalizationError(ancestor.to_path_buf(), e.to_string())
                })?;
                let mut target = canonical_ancestor;
                for part in remaining_parts.into_iter().rev() {
                    target.push(part);
                }
                if let Some(name) = path.file_name() {
                    target.push(name);
                }
                target
            } else {
                normalize_components(path)
            }
        };

        if self.is_under_any_root(&canonical) {
            Ok(canonical)
        } else {
            Err(PathPolicyError::OutsideRoots(canonical))
        }
    }

    /// Checks if a path is located under any of the granted roots.
    pub fn is_allowed(&self, path: &Path) -> bool {
        self.check_read(path).is_ok()
    }

    fn is_under_any_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|r| path.starts_with(r))
    }
}

/// Normalizes path components without touching the filesystem.
fn normalize_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

/// Checks if a string or its stem matches a Windows reserved device name.
fn is_reserved_device_name(name: &str) -> bool {
    let stem = if let Some(dot_idx) = name.find('.') {
        &name[..dot_idx]
    } else {
        name
    };

    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    reserved.iter().any(|&r| stem.eq_ignore_ascii_case(r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_traversal_rejection() {
        assert!(matches!(
            PathPolicy::check_syntax(Path::new(r"..\..\Windows\win.ini")),
            Err(PathPolicyError::TraversalDetected(_))
        ));
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("docs/../../secret.txt")),
            Err(PathPolicyError::TraversalDetected(_))
        ));
    }

    #[test]
    fn test_unc_rejection() {
        assert!(matches!(
            PathPolicy::check_syntax(Path::new(r"\\server\share\file.pdf")),
            Err(PathPolicyError::UncPathForbidden(_))
        ));
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("//server/share/file.pdf")),
            Err(PathPolicyError::UncPathForbidden(_))
        ));
    }

    #[test]
    fn test_reserved_device_names() {
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("CON")),
            Err(PathPolicyError::ReservedDeviceName(_))
        ));
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("nul.pdf")),
            Err(PathPolicyError::ReservedDeviceName(_))
        ));
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("folder/aux.txt")),
            Err(PathPolicyError::ReservedDeviceName(_))
        ));
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("com1")),
            Err(PathPolicyError::ReservedDeviceName(_))
        ));
    }

    #[test]
    fn test_trailing_dot_or_space() {
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("file.pdf.")),
            Err(PathPolicyError::TrailingDotOrSpace(_))
        ));
        assert!(matches!(
            PathPolicy::check_syntax(Path::new("file.pdf ")),
            Err(PathPolicyError::TrailingDotOrSpace(_))
        ));
    }

    #[test]
    fn test_granted_roots_containment() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("document.pdf");
        std::fs::write(&file_path, b"%PDF-1.4 test").unwrap();

        let policy = PathPolicy::new(vec![dir.path().to_path_buf()]);

        // Inside root is allowed
        let res = policy.check_read(&file_path);
        assert!(res.is_ok(), "Expected path to be allowed: {:?}", res);

        // Outside root is rejected
        let other_dir = tempdir().unwrap();
        let other_file = other_dir.path().join("external.pdf");
        std::fs::write(&other_file, b"%PDF-1.4 other").unwrap();

        let outside_res = policy.check_read(&other_file);
        assert!(matches!(outside_res, Err(PathPolicyError::OutsideRoots(_))));
    }
}
