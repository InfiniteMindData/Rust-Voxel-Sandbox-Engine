//! Application-level error types.
//!
//! The engine uses a single `Result` alias with a boxed-display error for
//! application-level failures (I/O, display setup, configuration). Systems
//! with richer failure modes define their own typed errors internally and
//! convert at the boundary.

use std::fmt;

/// Convenience result alias for engine-level fallible operations.
pub type Result<T> = std::result::Result<T, EngineError>;

/// Top-level engine error.
#[derive(Debug)]
pub enum EngineError {
    /// An I/O operation failed (display socket, save files, ...).
    Io(std::io::Error),
    /// A configuration value was invalid.
    Config(String),
    /// An internal invariant that should never be violated was violated.
    /// This indicates a bug in the engine, not a user-facing condition.
    /// Construction sites appear as systems adopt this error type.
    #[allow(dead_code)]
    Invariant(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Io(e) => write!(f, "i/o error: {e}"),
            EngineError::Config(msg) => write!(f, "configuration error: {msg}"),
            EngineError::Invariant(msg) => {
                write!(f, "internal invariant violated (this is a bug): {msg}")
            }
        }
    }
}

impl std::error::Error for EngineError {}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e)
    }
}
