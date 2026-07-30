//! Browser Launcher
//!
//! Handles Chrome discovery, launching with stealth flags, and binary patching.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Global counter for unique user data directories
static BROWSER_COUNTER: AtomicU64 = AtomicU64::new(0);

use crate::cdp::transport::launch_chrome;
use crate::cdp::types::{EmulationSetUserAgentOverride, UserAgentBrandVersion, UserAgentMetadata};
use crate::cdp::{Connection, Transport};
use crate::error::{Error, Result};
use crate::page::Page;
use crate::stealth::fingerprint::Fingerprint;
use crate::stealth::{build_evasion_script_for, find_chrome, ChromePatcher};
use crate::StealthConfig;

/// Stealth browser arguments (pre-built for zero allocation).
fn stealth_args(config: &StealthConfig, fingerprint: &Fingerprint) -> Vec<String> {
    let mut args = vec![
        // Core automation hiding
        "--disable-blink-features=AutomationControlled".into(),
        "--disable-features=IsolateOrigins,site-per-process,AutomationControlled,EnableAutomation"
            .into(),
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
        "--lang=en-US".into(),
        // Window size
        format!(
            "--window-size={},{}",
            config.viewport_width, config.viewport_height
        ),
        // Keep Chromium's virtual screen aligned with the viewport in new
        // headless mode; otherwise it reports a fixed 800x600 screen.
        format!(
            "--ozone-override-screen-size={},{}",
            config.viewport_width, config.viewport_height
        ),
    ];

    // User agent
    args.push(format!("--user-agent={}", fingerprint.user_agent));

    // Headless mode
    if config.headless {
        args.push("--headless=new".into());
    }

    // Proxy
    if let Some(ref proxy) = config.proxy {
        args.push(format!("--proxy-server={}", proxy));
    }

    // Extra user-supplied args (e.g. --use-fake-ui-for-media-stream)
    for arg in &config.extra_args {
        args.push(arg.clone());
    }

    args
}

/// Info about an open tab
#[derive(Debug, Clone)]
pub struct TabInfo {
    /// Target ID of the tab.
    pub id: String,
    /// Tab title.
    pub title: String,
    /// URL currently loaded in the tab.
    pub url: String,
}

/// Build the `Emulation.setUserAgentOverride` payload from the resolved fingerprint.
fn ua_override_for(fp: &Fingerprint) -> EmulationSetUserAgentOverride {
    let to_brands = |v: Vec<(String, String)>| -> Vec<UserAgentBrandVersion> {
        v.into_iter()
            .map(|(brand, version)| UserAgentBrandVersion { brand, version })
            .collect()
    };
    let architecture = if fp.webgl_renderer.contains("Apple M") {
        "arm"
    } else {
        "x86"
    };
    EmulationSetUserAgentOverride {
        user_agent: fp.user_agent.clone(),
        accept_language: Some("en-US,en;q=0.9".to_string()),
        platform: Some(fp.nav_platform().to_string()),
        user_agent_metadata: Some(UserAgentMetadata {
            brands: to_brands(fp.ch_brands()),
            full_version_list: to_brands(fp.ch_full_version_brands()),
            platform: fp.ch_platform().to_string(),
            platform_version: fp.platform_version.clone(),
            architecture: architecture.to_string(),
            model: String::new(),
            mobile: false,
            bitness: "64".to_string(),
            wow64: false,
        }),
    }
}

/// The main stealth browser
pub struct Browser {
    connection: Connection,
    config: Arc<StealthConfig>,
    /// User data directory (cleaned up on close; None when connecting to existing instance)
    user_data_dir: Option<PathBuf>,
    /// Resolved fingerprint (None in live-session mode).
    fingerprint: Option<Fingerprint>,
    /// Evasion script (cached)
    evasion_script: String,
}

impl Browser {
    /// Launch a new stealth browser with default config
    pub async fn launch() -> Result<Self> {
        Self::launch_with_config(StealthConfig::default()).await
    }

    /// Launch a new stealth browser with a visible window.
    pub async fn launch_visible() -> Result<Self> {
        Self::launch_with_config(StealthConfig::visible()).await
    }

    /// Launch a new visible stealth browser with debug logging behavior enabled.
    pub async fn launch_debug() -> Result<Self> {
        Self::launch_with_config(StealthConfig::debug()).await
    }

    /// Launch with the default stealth config after applying inline changes.
    ///
    /// This avoids constructing a full [`StealthConfig`] when only one or two
    /// options need to change.
    pub async fn launch_with(configure: impl FnOnce(&mut StealthConfig)) -> Result<Self> {
        let mut config = StealthConfig::default();
        configure(&mut config);
        Self::launch_with_config(config).await
    }

    /// Launch with custom config
    pub async fn launch_with_config(config: StealthConfig) -> Result<Self> {
        let config = Arc::new(config);

        // Ephemeral calls use a disposable profile. A caller-provided profile
        // is durable and must never be wiped by Eoka.
        let (user_data_dir, owns_user_data_dir) = match config.user_data_dir.as_deref() {
            Some(path) => {
                let path = PathBuf::from(path);
                std::fs::create_dir_all(&path)?;
                (path, false)
            }
            None => {
                let instance_id = BROWSER_COUNTER.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "eoka-browser-{}-{}",
                    std::process::id(),
                    instance_id
                ));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path)?;
                (path, true)
            }
        };

        // Clean up the temp user-data-dir on any failure past this point.
        let launch = async {
            let chrome_path = match &config.chrome_path {
                Some(p) => PathBuf::from(p),
                None => find_chrome()?,
            };

            // Patching a browser binary is not appropriate for a durable
            // user-owned profile. Keep that mode close to normal Chromium and
            // let its profile provide continuity instead of fresh spoofing.
            let chrome_path = if config.patch_binary && owns_user_data_dir {
                ChromePatcher::new(&chrome_path)?.get_patched_path()?
            } else {
                chrome_path
            };

            let detected_user_agent = if config.user_agent.is_none() {
                installed_chrome_user_agent(&chrome_path)
            } else {
                None
            };
            let fingerprint = Fingerprint::resolve_for_profile(
                config
                    .user_agent
                    .as_deref()
                    .or(detected_user_agent.as_deref()),
                config.timezone.as_deref(),
                (!owns_user_data_dir).then_some(user_data_dir.as_path()),
            )?;

            // Build args
            let mut args = stealth_args(&config, &fingerprint);
            args.push(format!("--user-data-dir={}", user_data_dir.display()));

            tracing::info!("Launching Chrome from {:?}", chrome_path);
            let (child, ws_url) = launch_chrome(&chrome_path, &args)?;

            let proxy_auth = match (&config.proxy_username, &config.proxy_password) {
                (Some(u), Some(p)) => Some((u.clone(), p.clone())),
                _ => None,
            };
            let transport =
                Transport::new_with_options(child, &ws_url, proxy_auth, config.cdp_timeout).await?;
            let connection = Connection::new(transport);

            let version = connection.version().await?;
            tracing::info!("Connected to Chrome: {}", version.product);

            Ok::<(Connection, Fingerprint), Error>((connection, fingerprint))
        }
        .await;

        let (connection, fingerprint) = match launch {
            Ok(result) => result,
            Err(e) => {
                if owns_user_data_dir {
                    let _ = std::fs::remove_dir_all(&user_data_dir);
                }
                return Err(e);
            }
        };

        let evasion_script = build_evasion_script_for(&config, &fingerprint);

        Ok(Self {
            connection,
            config,
            user_data_dir: owns_user_data_dir.then_some(user_data_dir),
            fingerprint: Some(fingerprint),
            evasion_script,
        })
    }

    /// Connect to an existing Chrome instance at the given WebSocket CDP URL.
    /// Obtain the URL from `curl http://localhost:9222/json/version` or use
    /// `Browser::connect_port` for HTTP discovery.
    ///
    /// Defaults to `StealthConfig::live()` so attaching to a user's real browser
    /// leaves their tabs untouched (no evasion script injection, full CDP access).
    pub async fn connect(ws_url: &str) -> Result<Self> {
        Self::connect_with_config(ws_url, StealthConfig::live()).await
    }

    /// Connect to an existing Chrome instance with a custom config.
    /// Allows customizing CDP timeout, evasion scripts, proxy auth, etc.
    pub async fn connect_with_config(ws_url: &str, config: StealthConfig) -> Result<Self> {
        let config = Arc::new(config);
        let transport = crate::cdp::transport::Transport::connect_with_options(
            ws_url,
            config.cdp_timeout,
            config.filter_cdp,
        )
        .await?;
        let connection = Connection::new(transport);
        let version = connection.version().await?;
        tracing::info!("Connected to Chrome: {}", version.product);
        // Skip the evasion script and fingerprint for live sessions.
        let (fingerprint, evasion_script) = if config.live_session {
            (None, String::new())
        } else {
            let fp = Fingerprint::resolve(config.user_agent.as_deref(), config.timezone.as_deref());
            let script = build_evasion_script_for(&config, &fp);
            (Some(fp), script)
        };
        Ok(Self {
            connection,
            config,
            user_data_dir: None,
            fingerprint,
            evasion_script,
        })
    }

    /// Discover the DevTools URL on `127.0.0.1:<port>` and connect.
    /// Equivalent to `curl http://127.0.0.1:<port>/json/version` then `Browser::connect`.
    pub async fn connect_port(port: u16) -> Result<Self> {
        let ws_url = crate::cdp::discover::discover_browser_ws("127.0.0.1", port)?;
        Self::connect(&ws_url).await
    }

    /// `connect_port` with a custom config.
    pub async fn connect_port_with_config(port: u16, config: StealthConfig) -> Result<Self> {
        let ws_url = crate::cdp::discover::discover_browser_ws("127.0.0.1", port)?;
        Self::connect_with_config(&ws_url, config).await
    }

    /// Set up a session with evasion scripts and proxy auth.
    /// Common logic shared by new_page, new_blank_page, and attach_page.
    /// In live-session mode, skips the evasion-script injection so we don't
    /// pollute the user's tab with extra `addScriptToEvaluateOnNewDocument`.
    async fn setup_session(&self, session: &crate::cdp::Session) -> Result<()> {
        session.page_enable().await?;

        if self.config.proxy_username.is_some() && self.config.proxy_password.is_some() {
            session.fetch_enable(true).await?;
        }

        if !self.config.live_session {
            if let Some(ref fp) = self.fingerprint {
                session.set_user_agent_full(ua_override_for(fp)).await?;
            }
            session
                .add_script_to_evaluate_on_new_document(&self.evasion_script)
                .await?;
        }

        Ok(())
    }

    /// Create a new page and navigate to URL
    pub async fn new_page(&self, url: &str) -> Result<Page> {
        let target_id = self
            .connection
            .create_target("about:blank", None, None)
            .await?;

        let session = self.connection.attach_to_target(&target_id).await?;
        self.setup_session(&session).await?;

        // Navigate to URL
        let nav_result = session.navigate(url, None).await?;
        if let Some(error) = nav_result.error_text {
            return Err(Error::Navigation(error));
        }

        // Brief settle time for the initial page load to start.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(Page::new(session, Arc::clone(&self.config)))
    }

    /// Create a new page without navigation (at about:blank)
    pub async fn new_blank_page(&self) -> Result<Page> {
        let target_id = self
            .connection
            .create_target("about:blank", None, None)
            .await?;

        let session = self.connection.attach_to_target(&target_id).await?;
        self.setup_session(&session).await?;

        Ok(Page::new(session, Arc::clone(&self.config)))
    }

    /// Get the browser version
    pub async fn version(&self) -> Result<String> {
        let v = self.connection.version().await?;
        Ok(v.product)
    }

    /// List all open tabs
    pub async fn tabs(&self) -> Result<Vec<TabInfo>> {
        let targets = self.connection.get_targets().await?;
        Ok(targets
            .into_iter()
            .filter(|t| t.r#type == "page")
            .map(|t| TabInfo {
                id: t.target_id,
                title: t.title,
                url: t.url,
            })
            .collect())
    }

    /// Attach to an existing browser target (e.g., a popup opened by window.open()).
    /// Use `tabs()` to discover popup target IDs, then call this to get a Page handle.
    pub async fn attach_page(&self, target_id: &str) -> Result<Page> {
        let session = self.connection.attach_to_target(target_id).await?;
        self.setup_session(&session).await?;
        Ok(Page::new(session, Arc::clone(&self.config)))
    }

    /// Activate (focus) a tab by target ID
    pub async fn activate_tab(&self, target_id: &str) -> Result<()> {
        self.connection.activate_target(target_id).await
    }

    /// Close a specific tab by target ID
    pub async fn close_tab(&self, target_id: &str) -> Result<()> {
        self.connection.close_target(target_id).await?;
        Ok(())
    }

    /// Close the browser. In live-session mode, this is equivalent to
    /// `disconnect()` — we never send `Browser.close` to a Chrome we don't own.
    pub async fn close(self) -> Result<()> {
        if self.config.live_session {
            // Don't kill the user's Chrome; just drop the connection.
            self.connection.transport().close().await?;
        } else {
            self.connection.close().await?;
        }

        // Clean up user data directory (None when connecting).
        if let Some(ref dir) = self.user_data_dir {
            let _ = std::fs::remove_dir_all(dir);
        }

        Ok(())
    }

    /// Drop the WebSocket without sending `Browser.close`. Use this when
    /// connected to a user-owned browser to disconnect without killing it.
    pub async fn disconnect(self) -> Result<()> {
        self.connection.transport().close().await
    }
}

/// Read Chrome's executable version and derive a coherent native UA. Failure
/// is intentionally non-fatal: callers fall back to a current fingerprint.
fn installed_chrome_user_agent(chrome_path: &std::path::Path) -> Option<String> {
    let output = Command::new(chrome_path).arg("--version").output().ok()?;
    let version = String::from_utf8_lossy(&output.stdout);
    let full_version = version.split_whitespace().find(|part| {
        let pieces: Vec<_> = part.split('.').collect();
        pieces.len() == 4
            && pieces
                .iter()
                .all(|piece| !piece.is_empty() && piece.bytes().all(|byte| byte.is_ascii_digit()))
    })?;
    Fingerprint::native_chrome_user_agent(full_version)
}

impl Drop for Browser {
    fn drop(&mut self) {
        // Best-effort cleanup of the temp user-data-dir if close() wasn't called.
        if let Some(ref dir) = self.user_data_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
