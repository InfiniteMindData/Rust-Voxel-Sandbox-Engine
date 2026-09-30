//! Minimal dependency-free leveled logging.
//!
//! Logs go to stderr with a `[LEVEL target]` prefix. The maximum level is a
//! process-wide atomic so hot code can cheaply early-out before formatting.
//!
//! A real `tracing` dependency can replace this module later without touching
//! call sites, since call sites only use the macros defined here.

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

/// Log severity levels, ordered from most to least severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    /// Parses a level name as used by the `BLOCKSCAPE_LOG` environment
    /// variable. Unknown names resolve to [`Level::Info`].
    pub fn from_env_str(name: &str) -> Level {
        match name.to_ascii_lowercase().as_str() {
            "error" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "debug" => Level::Debug,
            "trace" => Level::Trace,
            _ => Level::Info,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Sets the maximum level that will be emitted.
pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Returns whether messages of `level` are currently emitted.
pub fn enabled(level: Level) -> bool {
    (level as u8) <= MAX_LEVEL.load(Ordering::Relaxed)
}

/// Core logging function used by the macros in this module.
pub fn log(level: Level, target: &str, args: fmt::Arguments<'_>) {
    if enabled(level) {
        eprintln!("[{} {}] {args}", level.as_str(), target);
    }
}

/// Emits an error-level log message.
#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::engine::logging::log($crate::engine::logging::Level::Error, $target, format_args!($($arg)*))
    };
}

/// Emits a warning-level log message.
#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::engine::logging::log($crate::engine::logging::Level::Warn, $target, format_args!($($arg)*))
    };
}

/// Emits an info-level log message.
#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::engine::logging::log($crate::engine::logging::Level::Info, $target, format_args!($($arg)*))
    };
}

/// Emits a debug-level log message.
#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        $crate::engine::logging::log($crate::engine::logging::Level::Debug, $target, format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_parsing_has_sane_fallback() {
        assert_eq!(Level::from_env_str("error"), Level::Error);
        assert_eq!(Level::from_env_str("TRACE"), Level::Trace);
        assert_eq!(Level::from_env_str("nonsense"), Level::Info);
    }

    #[test]
    fn level_ordering_is_severity_order() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert!(Level::Debug < Level::Trace);
    }
}
