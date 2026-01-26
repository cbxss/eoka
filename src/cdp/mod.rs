//! Minimal CDP (Chrome DevTools Protocol) Client
//!
//! A hand-crafted CDP implementation with built-in stealth filtering.
//! Only ~20 CDP commands are implemented - just what's needed for browser automation.
//!
//! ## Stealth Features
//!
//! - Blocks detectable commands like `Runtime.enable`
//! - Warns on risky commands like `Emulation.setUserAgentOverride`
//! - Uses pipe transport (less detectable than WebSocket)

pub mod connection;
pub mod transport;
pub mod types;

pub use connection::{Connection, Session};
pub use transport::Transport;
pub use types::*;
