//! Error types for eoka

use thiserror::Error;

/// Result type for eoka operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for eoka
#[derive(Debug, Error)]
pub enum Error {
    /// Failed to launch Chrome
    #[error("Failed to launch Chrome: {0}")]
    Launch(String),

    /// Transport error
    #[error(fmt = transport_fmt)]
    Transport {
        context: String,
        #[source]
        source: Option<std::io::Error>,
    },

    /// CDP protocol error
    #[error("CDP error in {method}: {message} (code {code})")]
    Cdp {
        method: String,
        code: i64,
        message: String,
    },

    /// CDP error without method context (for simple cases)
    #[error("CDP error: {0}")]
    CdpSimple(String),

    /// Navigation error
    #[error("Navigation error: {0}")]
    Navigation(String),

    /// Element not found in DOM
    #[error("Element not found: {0}")]
    ElementNotFound(String),

    /// Element exists in DOM but is not visible/rendered
    #[error("Element not visible: '{selector}' exists in DOM but is not rendered (hidden, display:none, or off-screen)")]
    ElementNotVisible { selector: String },

    /// Timeout
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Decode error (e.g., base64)
    #[error("Decode error: {0}")]
    Decode(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Chrome not found
    #[error("Chrome not found")]
    ChromeNotFound,

    /// Binary patching error
    #[error("Patching error in {operation}: {message}")]
    Patching { operation: String, message: String },

    /// Retry exhausted
    #[error("Retry exhausted after {attempts} attempts: {last_error}")]
    RetryExhausted { attempts: u32, last_error: String },
}

/// Display for the `Transport` variant, including the io error cause when present.
fn transport_fmt(
    context: &str,
    source: &Option<std::io::Error>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    match source {
        Some(err) => write!(f, "Transport error: {}: {}", context, err),
        None => write!(f, "Transport error: {}", context),
    }
}

impl Error {
    /// Create a transport error with context
    pub fn transport(context: impl Into<String>) -> Self {
        Self::Transport {
            context: context.into(),
            source: None,
        }
    }

    /// Create a transport error with IO source
    pub fn transport_io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Transport {
            context: context.into(),
            source: Some(source),
        }
    }

    /// Create a CDP error with full context
    pub fn cdp(method: impl Into<String>, code: i64, message: impl Into<String>) -> Self {
        Self::Cdp {
            method: method.into(),
            code,
            message: message.into(),
        }
    }

    /// Create a patching error
    pub fn patching(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Patching {
            operation: operation.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;

    #[test]
    fn test_transport_io_display_includes_context_and_cause() {
        let err = Error::transport_io(
            "Failed to connect",
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "boom"),
        );
        let msg = format!("{}", err);
        // Both the context and the underlying io cause must appear.
        assert!(
            msg.contains("Failed to connect"),
            "missing context in: {msg}"
        );
        assert!(msg.contains("boom"), "missing io cause in: {msg}");
    }

    #[test]
    fn test_transport_io_exposes_source() {
        let err = Error::transport_io(
            "Failed to connect",
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "boom"),
        );
        assert!(
            err.source().is_some(),
            "transport_io error should expose its io source"
        );
    }

    #[test]
    fn test_transport_display_plain_message() {
        let err = Error::transport("plain msg");
        let msg = format!("{}", err);
        assert!(msg.contains("plain msg"), "missing message in: {msg}");
        // No source for the context-only constructor.
        assert!(err.source().is_none());
    }
}
