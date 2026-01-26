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
    #[error("Transport error: {context}")]
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

    /// Element not found
    #[error("Element not found: {0}")]
    ElementNotFound(String),

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
    Patching {
        operation: String,
        message: String,
    },
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
