//! # Eoka
//!
//! Undetectable browser automation for AI agents.
//!
//! Eoka is a minimal, fast stealth browser library built from scratch. It uses a custom
//! CDP (Chrome DevTools Protocol) implementation with built-in stealth filtering to avoid detection.
//!
//! ## Features
//!
//! - **Stealth by Default** - Binary patching, JavaScript evasions, human simulation
//! - **Minimal Dependencies** - ~10 crates total, no chromiumoxide
//! - **AI-Agent Optimized** - PageState, element indexing, text extraction
//! - **Fast** - Lazy evasion scripts, mmap patching, stack-allocated paths
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use eoka::Browser;
//!
//! #[tokio::main]
//! async fn main() -> eoka::Result<()> {
//!     // Launch browser (patches Chrome, applies evasions)
//!     let browser = Browser::launch().await?;
//!
//!     // Create page and navigate
//!     let page = browser.new_page("https://example.com").await?;
//!
//!     // Human-like interactions
//!     page.human_click("#button").await?;
//!     page.human_type("#input", "hello world").await?;
//!
//!     // Screenshot
//!     let png = page.screenshot().await?;
//!     std::fs::write("screenshot.png", png)?;
//!
//!     browser.close().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Configuration
//!
//! ```rust,no_run
//! use eoka::{Browser, StealthConfig};
//!
//! # #[tokio::main]
//! # async fn main() -> eoka::Result<()> {
//! let config = StealthConfig {
//!     headless: true,
//!     patch_binary: true,
//!     webgl_spoof: true,
//!     canvas_spoof: true,
//!     ..Default::default()
//! };
//!
//! let browser = Browser::launch_with_config(config).await?;
//! # Ok(())
//! # }
//! ```

pub mod annotate;
pub mod browser;
pub mod cdp;
pub mod error;
pub mod network;
pub mod page;
pub mod session;
pub mod stealth;

// Re-exports
pub use browser::Browser;
pub use error::{Error, Result};
pub use page::{Element, Page, CapturedRequest, ResponseBody};
pub use session::{BrowserSession, SessionCookie};
pub use stealth::{HumanSpeed, Fingerprint};
pub use network::{NetworkEvent, NetworkWatcher};
pub use annotate::{AnnotationConfig, AnnotationError, InteractiveElement, annotate_screenshot};

/// Configuration for stealth features
#[derive(Debug, Clone)]
pub struct StealthConfig {
    /// Spoof WebGL renderer/vendor
    pub webgl_spoof: bool,
    /// Spoof canvas fingerprint
    pub canvas_spoof: bool,
    /// Spoof audio fingerprint
    pub audio_spoof: bool,
    /// Use human-like mouse movements
    pub human_mouse: bool,
    /// Use human-like typing
    pub human_typing: bool,
    /// Custom user agent (None = random realistic)
    pub user_agent: Option<String>,
    /// Headless mode
    pub headless: bool,
    /// Path to Chrome/Chromium binary
    pub chrome_path: Option<String>,
    /// Patch Chrome binary to bypass detection
    pub patch_binary: bool,
    /// Viewport width
    pub viewport_width: u32,
    /// Viewport height
    pub viewport_height: u32,
}

impl Default for StealthConfig {
    fn default() -> Self {
        Self {
            webgl_spoof: true,
            canvas_spoof: true,
            audio_spoof: true,
            human_mouse: true,
            human_typing: true,
            user_agent: None,
            headless: true,
            chrome_path: None,
            patch_binary: true,
            viewport_width: 1920,
            viewport_height: 1080,
        }
    }
}

impl StealthConfig {
    /// Create a minimal config (no spoofing, no patching)
    pub fn minimal() -> Self {
        Self {
            webgl_spoof: false,
            canvas_spoof: false,
            audio_spoof: false,
            human_mouse: false,
            human_typing: false,
            user_agent: None,
            headless: false,
            chrome_path: None,
            patch_binary: false,
            viewport_width: 1920,
            viewport_height: 1080,
        }
    }

    /// Create a visible (non-headless) config
    pub fn visible() -> Self {
        Self {
            headless: false,
            ..Default::default()
        }
    }
}
