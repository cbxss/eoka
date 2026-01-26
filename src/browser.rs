//! Browser Launcher
//!
//! Handles Chrome discovery, launching with stealth flags, and binary patching.

use std::path::PathBuf;
use std::sync::Arc;

use crate::cdp::{Connection, Transport};
use crate::cdp::transport::launch_chrome;
use crate::error::{Error, Result};
use crate::page::Page;
use crate::stealth::{build_evasion_script, find_chrome, random_user_agent, ChromePatcher};
use crate::StealthConfig;

/// Stealth browser arguments (pre-built for zero allocation)
fn stealth_args(config: &StealthConfig) -> Vec<String> {
    let mut args = vec![
        // Core automation hiding
        "--disable-blink-features=AutomationControlled".into(),
        "--disable-automation".into(),
        "--disable-features=IsolateOrigins,site-per-process,AutomationControlled,EnableAutomation".into(),
        "--enable-features=NetworkService,NetworkServiceInProcess".into(),
        // Additional flags to hide automation
        "--disable-infobars".into(),
        "--disable-dev-shm-usage".into(),
        "--disable-ipc-flooding-protection".into(),
        "--disable-renderer-backgrounding".into(),
        "--disable-background-timer-throttling".into(),
        "--disable-backgrounding-occluded-windows".into(),
        // Make browser look natural
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--no-sandbox".into(),
        "--disable-extensions-except=".into(),
        "--disable-default-apps".into(),
        "--disable-component-extensions-with-background-pages".into(),
        "--disable-hang-monitor".into(),
        "--disable-popup-blocking".into(),
        "--disable-prompt-on-repost".into(),
        "--disable-sync".into(),
        "--disable-translate".into(),
        "--metrics-recording-only".into(),
        "--safebrowsing-disable-auto-update".into(),
        "--disable-client-side-phishing-detection".into(),
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
        "--excludeSwitches=enable-automation".into(),
        // Window size
        format!("--window-size={},{}", config.viewport_width, config.viewport_height),
    ];

    // User agent
    let user_agent = config.user_agent.clone().unwrap_or_else(random_user_agent);
    args.push(format!("--user-agent={}", user_agent));

    // Headless mode
    if config.headless {
        args.push("--headless=new".into());
    }

    args
}

/// The main stealth browser
pub struct Browser {
    connection: Connection,
    config: Arc<StealthConfig>,
    /// User data directory (cleaned up on close)
    user_data_dir: PathBuf,
    /// Evasion script (cached)
    evasion_script: String,
}

impl Browser {
    /// Launch a new stealth browser with default config
    pub async fn launch() -> Result<Self> {
        Self::launch_with_config(StealthConfig::default()).await
    }

    /// Launch with custom config
    pub async fn launch_with_config(config: StealthConfig) -> Result<Self> {
        let config = Arc::new(config);

        // Create unique user data directory
        let user_data_dir = std::env::temp_dir()
            .join(format!("eoka-browser-{}", std::process::id()));

        // Clean up any stale data
        let _ = std::fs::remove_dir_all(&user_data_dir);
        std::fs::create_dir_all(&user_data_dir)?;

        // Find Chrome path
        let chrome_path = match &config.chrome_path {
            Some(p) => PathBuf::from(p),
            None => find_chrome()?,
        };

        // Optionally patch the binary
        let chrome_path = if config.patch_binary {
            let patcher = ChromePatcher::new(&chrome_path)?;
            patcher.get_patched_path()?
        } else {
            chrome_path
        };

        // Build args
        let mut args = stealth_args(&config);
        args.push(format!("--user-data-dir={}", user_data_dir.display()));

        // Launch Chrome
        tracing::info!("Launching Chrome from {:?}", chrome_path);
        let (child, ws_url) = launch_chrome(&chrome_path, &args)?;

        // Create transport and connection
        let transport = Transport::new(child, &ws_url)?;
        let connection = Connection::new(transport);

        // Get browser version
        let version = connection.version().await?;
        tracing::info!("Connected to Chrome: {}", version.product);

        // Build evasion script
        let evasion_script = build_evasion_script(&config);

        Ok(Self {
            connection,
            config,
            user_data_dir,
            evasion_script,
        })
    }

    /// Create a new page and navigate to URL
    pub async fn new_page(&self, url: &str) -> Result<Page> {
        // Create a new target (window size is set via --window-size Chrome arg)
        let target_id = self.connection.create_target(
            "about:blank",
            None,
            None,
        ).await?;

        // Attach to the target
        let session = self.connection.attach_to_target(&target_id).await?;

        // Enable page events
        session.page_enable().await?;

        // Inject evasion scripts BEFORE navigation
        session.add_script_to_evaluate_on_new_document(&self.evasion_script).await?;

        // Navigate to URL
        let nav_result = session.navigate(url).await?;
        if let Some(error) = nav_result.error_text {
            return Err(Error::Navigation(error));
        }

        // Wait a bit for page to load
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(Page::new(session, Arc::clone(&self.config)))
    }

    /// Create a new page without navigation (at about:blank)
    pub async fn new_blank_page(&self) -> Result<Page> {
        let target_id = self.connection.create_target(
            "about:blank",
            None,
            None,
        ).await?;

        let session = self.connection.attach_to_target(&target_id).await?;
        session.page_enable().await?;
        session.add_script_to_evaluate_on_new_document(&self.evasion_script).await?;

        Ok(Page::new(session, Arc::clone(&self.config)))
    }

    /// Get the browser version
    pub async fn version(&self) -> Result<String> {
        let v = self.connection.version().await?;
        Ok(v.product)
    }

    /// Close the browser
    pub async fn close(self) -> Result<()> {
        self.connection.close().await?;

        // Clean up user data directory
        let _ = std::fs::remove_dir_all(&self.user_data_dir);

        Ok(())
    }
}
