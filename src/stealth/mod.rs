//! Stealth Layer
//!
//! All the anti-detection components:
//! - Binary patcher (Aho-Corasick based)
//! - JavaScript evasion scripts
//! - Human-like interaction simulation
//! - Fingerprint generation

pub mod evasions;
pub mod fingerprint;
pub mod human;
pub mod patcher;

pub use evasions::{build_evasion_script, full_evasion_script};
pub use fingerprint::{random_user_agent, Fingerprint, Platform};
pub use human::{hesitate, reading_pause, Human, HumanSpeed};
pub use patcher::{find_chrome, ChromePatcher};
