//! Minimal CDP (Chrome DevTools Protocol) Client
//!
//! A hand-crafted CDP implementation with built-in stealth filtering.
//! Only ~20 CDP commands are implemented - just what's needed for browser automation.
//!
//! ## Stealth Features
//!
//! - Blocks detectable commands like `Runtime.enable` (toggle via `Transport::connect_with_options`)
//! - Warns on risky commands like `Emulation.setUserAgentOverride`
//! - Uses a single WebSocket transport over TCP

pub mod connection;
pub mod discover;
pub mod transport;
pub mod types;

pub use connection::{Connection, Session};
pub use discover::{auto_connect, discover_browser_ws, discover_pages, PageTarget};
pub use transport::Transport;
pub use types::*;
