//! Browser fingerprint generation
//!
//! Generates realistic, internally consistent browser fingerprints.
//!
//! Ephemeral browsers may use a random identity. A browser backed by a durable
//! user-data directory must not: changing WebGL, CPU, or memory values between
//! visits makes a supposedly persistent login look like a new device.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

const PERSISTED_FINGERPRINT_FILE: &str = ".eoka-fingerprint.json";

/// Recent Chrome versions as (major, full) pairs.
const CHROME_VERSIONS: &[(&str, &str)] = &[
    ("133", "133.0.6943.142"),
    ("134", "134.0.6998.118"),
    ("135", "135.0.7049.96"),
    ("136", "136.0.7103.114"),
    ("137", "137.0.7151.119"),
    ("138", "138.0.7204.101"),
    ("139", "139.0.7258.139"),
    ("140", "140.0.7312.86"),
    ("150", "150.0.7871.182"),
];

/// macOS client-hint platform versions (UA string is frozen at 10_15_7).
const MACOS_CH_VERSIONS: &[&str] = &["13.6.0", "14.4.1", "14.5.0", "14.6.1", "15.0.0", "15.1.0"];

/// Windows client-hint platform versions.
const WINDOWS_CH_VERSIONS: &[&str] = &["10.0.0", "15.0.0"];

/// Linux client-hint platform versions. Chromium exposes a kernel-style
/// version here rather than advertising the distribution release.
const LINUX_CH_VERSIONS: &[&str] = &["5.15.0", "6.8.0"];

/// WebGL renderers for Mac
const WEBGL_RENDERERS_MAC: &[&str] = &[
    "ANGLE (Apple, Apple M1 Pro, OpenGL 4.1)",
    "ANGLE (Apple, Apple M1, OpenGL 4.1)",
    "ANGLE (Apple, Apple M2, OpenGL 4.1)",
    "ANGLE (Apple, Apple M3, OpenGL 4.1)",
    "ANGLE (Intel, Intel Iris Pro Graphics 6200, OpenGL 4.1)",
    "ANGLE (Intel, Intel UHD Graphics 630, OpenGL 4.1)",
];

/// WebGL renderers for Windows
const WEBGL_RENDERERS_WINDOWS: &[&str] = &[
    "ANGLE (NVIDIA, NVIDIA GeForce RTX 3080 Direct3D11 vs_5_0 ps_5_0, D3D11)",
    "ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 Direct3D11 vs_5_0 ps_5_0, D3D11)",
    "ANGLE (NVIDIA, NVIDIA GeForce GTX 1080 Direct3D11 vs_5_0 ps_5_0, D3D11)",
    "ANGLE (AMD, AMD Radeon RX 6800 XT Direct3D11 vs_5_0 ps_5_0, D3D11)",
    "ANGLE (Intel, Intel(R) UHD Graphics 770 Direct3D11 vs_5_0 ps_5_0, D3D11)",
];

/// Common Chromium-on-Linux software renderer. Native Linux profiles leave
/// WebGL untouched; this is only used when callers explicitly request a
/// synthetic Linux profile.
const WEBGL_RENDERERS_LINUX: &[&str] = &[
    "ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (LLVM 16.0.0) (0x0000C0DE)), SwiftShader driver)",
];

/// Platform-specific screen resolutions.
const SCREEN_RESOLUTIONS_MAC: &[(u32, u32)] = &[
    (1440, 900),
    (1680, 1050),
    (2560, 1600),
    (1920, 1080),
    (3024, 1964), // MacBook Pro 14"
    (3456, 2234), // MacBook Pro 16"
];
const SCREEN_RESOLUTIONS_WINDOWS: &[(u32, u32)] = &[
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
    (1366, 768),
    (1536, 864),
    (1600, 900),
];
const SCREEN_RESOLUTIONS_LINUX: &[(u32, u32)] =
    &[(1920, 1080), (2560, 1440), (1366, 768), (1600, 900)];

/// Pick a random element from a slice
fn choose<T>(slice: &[T]) -> &T {
    &slice[fastrand::usize(..slice.len())]
}

/// WebGL vendor, WebGL renderer and client-hint platform version for a platform.
fn platform_surfaces(platform: Platform) -> (&'static str, String, String) {
    match platform {
        Platform::MacOS => (
            "Google Inc. (Apple)",
            choose(WEBGL_RENDERERS_MAC).to_string(),
            choose(MACOS_CH_VERSIONS).to_string(),
        ),
        Platform::Windows => (
            "Google Inc. (NVIDIA)",
            choose(WEBGL_RENDERERS_WINDOWS).to_string(),
            choose(WINDOWS_CH_VERSIONS).to_string(),
        ),
        Platform::Linux => (
            "Google Inc. (Google)",
            choose(WEBGL_RENDERERS_LINUX).to_string(),
            choose(LINUX_CH_VERSIONS).to_string(),
        ),
    }
}

/// A plausible screen resolution for the platform.
fn platform_screen(platform: Platform) -> (u32, u32) {
    match platform {
        Platform::MacOS => *choose(SCREEN_RESOLUTIONS_MAC),
        Platform::Windows => *choose(SCREEN_RESOLUTIONS_WINDOWS),
        Platform::Linux => *choose(SCREEN_RESOLUTIONS_LINUX),
    }
}

/// The UA string for a platform + Chrome full version.
fn ua_for(platform: Platform, chrome_full_version: &str) -> String {
    match platform {
        Platform::MacOS => format!(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36",
            chrome_full_version
        ),
        Platform::Windows => format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36",
            chrome_full_version
        ),
        Platform::Linux => format!(
            "Mozilla/5.0 (X11; Linux {}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36",
            native_linux_arch(), chrome_full_version
        ),
    }
}

/// Generate a random realistic user agent.
pub fn random_user_agent() -> String {
    Fingerprint::random().user_agent
}

/// Browser fingerprint data — a single coherent identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Full User-Agent string.
    pub user_agent: String,
    /// Operating system platform.
    pub platform: Platform,
    /// Chrome major version, e.g. "140" (drives `Sec-CH-UA` brand versions).
    pub chrome_major: String,
    /// Chrome full version, e.g. "140.0.7312.86".
    pub chrome_full_version: String,
    /// Client-hint OS version, e.g. "15.0.0".
    pub platform_version: String,
    /// Screen width in pixels.
    pub screen_width: u32,
    /// Screen height in pixels.
    pub screen_height: u32,
    /// Screen color depth in bits.
    pub color_depth: u8,
    /// `navigator.hardwareConcurrency` (logical CPU count).
    pub hardware_concurrency: u8,
    /// `navigator.deviceMemory` in gigabytes.
    pub device_memory: u8,
    /// IANA timezone name.
    pub timezone: String,
    /// `navigator.languages` list.
    pub languages: Vec<String>,
    /// Spoofed WebGL unmasked vendor.
    pub webgl_vendor: String,
    /// Spoofed WebGL unmasked renderer.
    pub webgl_renderer: String,
    /// Stable canvas/audio perturbation seed for this identity.
    #[serde(default = "random_noise_seed")]
    pub noise_seed: u32,
}

/// Operating system platform for a fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    /// macOS.
    MacOS,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

impl Fingerprint {
    /// Build a platform-consistent UA for the actual Chrome binary Eoka is
    /// launching. This avoids advertising an obsolete Chrome release while
    /// the browser exposes a newer TLS and JavaScript surface.
    pub fn native_chrome_user_agent(chrome_full_version: &str) -> Option<String> {
        let major = chrome_full_version.split('.').next()?;
        if major.is_empty()
            || !major.bytes().all(|byte| byte.is_ascii_digit())
            || chrome_full_version
                .split('.')
                .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return None;
        }
        Some(ua_for(native_platform(), chrome_full_version))
    }

    /// Build a fully coherent identity for a platform + Chrome version.
    fn build(platform: Platform, chrome_major: String, chrome_full_version: String) -> Self {
        let (webgl_vendor, webgl_renderer, platform_version) = platform_surfaces(platform);
        let (screen_width, screen_height) = platform_screen(platform);
        Self {
            user_agent: ua_for(platform, &chrome_full_version),
            platform,
            chrome_major,
            chrome_full_version,
            platform_version,
            screen_width,
            screen_height,
            color_depth: 24,
            hardware_concurrency: *choose(&[4, 8, 10, 12, 16]),
            device_memory: *choose(&[8, 8, 16, 32]),
            timezone: choose(super::evasions::COMMON_TIMEZONES).to_string(),
            languages: vec!["en-US".to_string(), "en".to_string()],
            webgl_vendor: webgl_vendor.to_string(),
            webgl_renderer,
            noise_seed: random_noise_seed(),
        }
    }

    /// Generate a random, internally consistent fingerprint.
    pub fn random() -> Self {
        // 30% Mac, 70% Windows (roughly matches desktop traffic).
        let platform = if fastrand::f64() < 0.3 {
            Platform::MacOS
        } else {
            Platform::Windows
        };
        let (major, full) = *choose(CHROME_VERSIONS);
        Self::build(platform, major.to_string(), full.to_string())
    }

    /// Resolve a fingerprint, honoring an explicit UA override or a random identity.
    pub fn resolve(user_agent: Option<&str>, timezone: Option<&str>) -> Self {
        let mut fp = match user_agent {
            Some(ua) => {
                let platform = if ua.contains("Macintosh") || ua.contains("Mac OS X") {
                    Platform::MacOS
                } else if ua.contains("Linux") {
                    Platform::Linux
                } else {
                    Platform::Windows
                };
                let (major, full) = ua
                    .split("Chrome/")
                    .nth(1)
                    .and_then(|rest| rest.split(' ').next())
                    .filter(|v| !v.is_empty())
                    .map(|v| (v.split('.').next().unwrap_or(v).to_string(), v.to_string()))
                    .unwrap_or_else(|| {
                        let (m, f) = *choose(CHROME_VERSIONS);
                        (m.to_string(), f.to_string())
                    });
                let mut fp = Self::build(platform, major, full);
                fp.user_agent = ua.to_string();
                fp
            }
            None => Self::random(),
        };
        if let Some(tz) = timezone {
            fp.timezone = tz.to_string();
        }
        fp
    }

    /// Resolves an identity for a browser profile, storing it beside the
    /// profile on first use. Explicit UA/timezone values are allowed only when
    /// they agree with an existing identity; silently changing either would
    /// defeat the point of a durable profile.
    pub fn resolve_for_profile(
        user_agent: Option<&str>,
        timezone: Option<&str>,
        profile_dir: Option<&Path>,
    ) -> io::Result<Self> {
        let Some(profile_dir) = profile_dir else {
            return Ok(Self::resolve(user_agent, timezone));
        };

        let path = profile_dir.join(PERSISTED_FINGERPRINT_FILE);
        if path.exists() {
            let contents = fs::read(&path)?;
            let fingerprint: Self = serde_json::from_slice(&contents).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "invalid persisted browser identity {}: {error}",
                        path.display()
                    ),
                )
            })?;
            if let Some(user_agent) = user_agent {
                if user_agent != fingerprint.user_agent {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "configured user agent conflicts with persisted browser identity",
                    ));
                }
            }
            if let Some(timezone) = timezone {
                if timezone != fingerprint.timezone {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "configured timezone conflicts with persisted browser identity",
                    ));
                }
            }
            return Ok(fingerprint);
        }

        let fingerprint = Self::resolve(user_agent, timezone);
        let temporary = profile_dir.join(format!(
            "{PERSISTED_FINGERPRINT_FILE}.tmp-{}",
            std::process::id()
        ));
        let encoded = serde_json::to_vec_pretty(&fingerprint).map_err(io::Error::other)?;
        fs::write(&temporary, encoded)?;
        fs::rename(temporary, path)?;
        Ok(fingerprint)
    }

    /// `navigator.platform` value consistent with this fingerprint.
    pub fn nav_platform(&self) -> &'static str {
        match self.platform {
            Platform::MacOS => "MacIntel",
            Platform::Windows => "Win32",
            Platform::Linux => "Linux x86_64",
        }
    }

    /// `Sec-CH-UA-Platform` value ("macOS" / "Windows").
    pub fn ch_platform(&self) -> &'static str {
        match self.platform {
            Platform::MacOS => "macOS",
            Platform::Windows => "Windows",
            Platform::Linux => "Linux",
        }
    }

    /// Client-hint brand list `(brand, version)` using the major version.
    pub fn ch_brands(&self) -> Vec<(String, String)> {
        vec![
            ("Chromium".to_string(), self.chrome_major.clone()),
            ("Google Chrome".to_string(), self.chrome_major.clone()),
            ("Not?A_Brand".to_string(), "24".to_string()),
        ]
    }

    /// Full-version brand list `(brand, full_version)` for
    /// `Sec-CH-UA-Full-Version-List`.
    pub fn ch_full_version_brands(&self) -> Vec<(String, String)> {
        vec![
            ("Chromium".to_string(), self.chrome_full_version.clone()),
            (
                "Google Chrome".to_string(),
                self.chrome_full_version.clone(),
            ),
            ("Not?A_Brand".to_string(), "24.0.0.0".to_string()),
        ]
    }
}

fn random_noise_seed() -> u32 {
    fastrand::u32(1..=u32::MAX)
}

fn native_platform() -> Platform {
    #[cfg(target_os = "macos")]
    {
        Platform::MacOS
    }
    #[cfg(target_os = "windows")]
    {
        Platform::Windows
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        Platform::Linux
    }
}

fn native_linux_arch() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "aarch64"
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        "x86_64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_user_agent_format() {
        for _ in 0..20 {
            let ua = random_user_agent();
            assert!(ua.starts_with("Mozilla/5.0"));
            assert!(ua.contains("Chrome/"));
            assert!(ua.contains("Safari/537.36"));
        }
    }

    #[test]
    fn test_fingerprint_random_is_consistent() {
        for _ in 0..50 {
            let fp = Fingerprint::random();
            assert!(!fp.user_agent.is_empty());
            assert!(fp.screen_width > 0 && fp.screen_height > 0);
            assert!([4, 8, 10, 12, 16].contains(&fp.hardware_concurrency));
            assert!([8, 16, 32].contains(&fp.device_memory));
            match fp.platform {
                Platform::MacOS => {
                    assert!(fp.user_agent.contains("Macintosh"));
                    assert_eq!(fp.nav_platform(), "MacIntel");
                    assert_eq!(fp.ch_platform(), "macOS");
                    // Chrome freezes the macOS UA token at 10_15_7.
                    assert!(fp.user_agent.contains("Mac OS X 10_15_7"));
                }
                Platform::Windows => {
                    assert!(fp.user_agent.contains("Windows NT 10.0"));
                    assert_eq!(fp.nav_platform(), "Win32");
                    assert_eq!(fp.ch_platform(), "Windows");
                }
                Platform::Linux => {
                    unreachable!("random profiles currently target macOS or Windows")
                }
            }
            assert!(fp.user_agent.contains(&fp.chrome_full_version));
            assert!(fp.chrome_full_version.starts_with(&fp.chrome_major));
        }
    }

    #[test]
    fn test_resolve_respects_pinned_ua() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.7312.86 Safari/537.36";
        let fp = Fingerprint::resolve(Some(ua), None);
        assert_eq!(fp.user_agent, ua);
        assert_eq!(fp.platform, Platform::Windows);
        assert_eq!(fp.chrome_major, "140");
        assert_eq!(fp.nav_platform(), "Win32");
    }

    #[test]
    fn test_native_chrome_user_agent_uses_supplied_version() {
        let ua = Fingerprint::native_chrome_user_agent("150.0.7871.182").unwrap();
        assert!(ua.contains("Chrome/150.0.7871.182"));
        assert!(Fingerprint::native_chrome_user_agent("not-a-version").is_none());
    }

    #[test]
    fn test_profile_identity_is_stable_across_launches() {
        let dir = std::env::temp_dir().join(format!(
            "eoka-fingerprint-test-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = Fingerprint::resolve_for_profile(None, Some("America/Los_Angeles"), Some(&dir))
            .unwrap();
        let second =
            Fingerprint::resolve_for_profile(None, Some("America/Los_Angeles"), Some(&dir))
                .unwrap();
        assert_eq!(first.user_agent, second.user_agent);
        assert_eq!(first.hardware_concurrency, second.hardware_concurrency);
        assert_eq!(first.device_memory, second.device_memory);
        assert_eq!(first.webgl_renderer, second.webgl_renderer);
        assert_eq!(first.noise_seed, second.noise_seed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_profile_identity_rejects_conflicting_user_agent() {
        let dir = std::env::temp_dir().join(format!(
            "eoka-fingerprint-test-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Fingerprint::resolve_for_profile(Some("Mozilla/5.0 first"), None, Some(&dir)).unwrap();
        assert!(
            Fingerprint::resolve_for_profile(Some("Mozilla/5.0 second"), None, Some(&dir)).is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Over many samples, every random fingerprint must be internally coherent:
    // the platform, UA string, nav_platform, ch_platform, and Chrome version
    // fields all agree with one another.
    #[test]
    fn test_random_is_internally_coherent_over_samples() {
        for _ in 0..200 {
            let fp = Fingerprint::random();
            match fp.platform {
                Platform::MacOS => {
                    assert!(
                        fp.user_agent.contains("Macintosh"),
                        "mac UA missing Macintosh: {}",
                        fp.user_agent
                    );
                    assert!(
                        fp.user_agent.contains("Mac OS X 10_15_7"),
                        "mac UA missing frozen OS token: {}",
                        fp.user_agent
                    );
                    assert_eq!(fp.nav_platform(), "MacIntel");
                    assert_eq!(fp.ch_platform(), "macOS");
                }
                Platform::Windows => {
                    assert!(
                        fp.user_agent.contains("Windows NT 10.0"),
                        "windows UA missing NT token: {}",
                        fp.user_agent
                    );
                    assert_eq!(fp.nav_platform(), "Win32");
                    assert_eq!(fp.ch_platform(), "Windows");
                }
                Platform::Linux => {
                    unreachable!("random profiles currently target macOS or Windows")
                }
            }
            // The UA always advertises the full Chrome version, and the full
            // version always begins with the major.
            assert!(
                fp.user_agent.contains(&fp.chrome_full_version),
                "UA missing full version {}: {}",
                fp.chrome_full_version,
                fp.user_agent
            );
            assert!(
                fp.chrome_full_version.starts_with(&fp.chrome_major),
                "full version {} does not start with major {}",
                fp.chrome_full_version,
                fp.chrome_major
            );
        }
    }

    // Regression: screen resolutions must be partitioned by platform. A Windows
    // fingerprint must never surface a Mac-only resolution, and vice versa.
    #[test]
    fn test_screen_resolution_partitioned_by_platform() {
        // Resolutions unique to each platform's list.
        let mac_only = [(3024u32, 1964u32), (3456, 2234)];
        let windows_only = [(1366u32, 768u32), (1536, 864)];

        // Sanity-check the assumptions against the actual consts.
        for res in &mac_only {
            assert!(SCREEN_RESOLUTIONS_MAC.contains(res));
            assert!(!SCREEN_RESOLUTIONS_WINDOWS.contains(res));
        }
        for res in &windows_only {
            assert!(SCREEN_RESOLUTIONS_WINDOWS.contains(res));
            assert!(!SCREEN_RESOLUTIONS_MAC.contains(res));
        }

        for _ in 0..300 {
            let fp = Fingerprint::random();
            let res = (fp.screen_width, fp.screen_height);
            match fp.platform {
                Platform::Windows => {
                    assert!(
                        !mac_only.contains(&res),
                        "windows fingerprint reported mac-only resolution {:?}",
                        res
                    );
                    assert!(
                        SCREEN_RESOLUTIONS_WINDOWS.contains(&res),
                        "windows resolution {:?} not in windows list",
                        res
                    );
                }
                Platform::MacOS => {
                    assert!(
                        !windows_only.contains(&res),
                        "mac fingerprint reported windows-only resolution {:?}",
                        res
                    );
                    assert!(
                        SCREEN_RESOLUTIONS_MAC.contains(&res),
                        "mac resolution {:?} not in mac list",
                        res
                    );
                }
                Platform::Linux => {
                    unreachable!("random profiles currently target macOS or Windows")
                }
            }
        }
    }

    #[test]
    fn test_resolve_windows_ua_is_coherent() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.7312.86 Safari/537.36";
        let fp = Fingerprint::resolve(Some(ua), None);
        assert_eq!(fp.platform, Platform::Windows);
        assert_eq!(fp.chrome_major, "140");
        assert_eq!(fp.user_agent, ua, "explicit UA must be preserved verbatim");
        assert!(
            WEBGL_RENDERERS_WINDOWS.contains(&fp.webgl_renderer.as_str()),
            "windows fp got non-windows renderer: {}",
            fp.webgl_renderer
        );
    }

    #[test]
    fn test_resolve_mac_ua_is_coherent() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.7312.86 Safari/537.36";
        let fp = Fingerprint::resolve(Some(ua), None);
        assert_eq!(fp.platform, Platform::MacOS);
        assert_eq!(fp.chrome_major, "140");
        assert_eq!(fp.user_agent, ua, "explicit UA must be preserved verbatim");
        assert_eq!(fp.nav_platform(), "MacIntel");
        assert!(
            WEBGL_RENDERERS_MAC.contains(&fp.webgl_renderer.as_str()),
            "mac fp got non-mac renderer: {}",
            fp.webgl_renderer
        );
    }

    #[test]
    fn test_resolve_linux_ua_is_coherent() {
        let ua = "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.7871.186 Safari/537.36";
        let fp = Fingerprint::resolve(Some(ua), Some("America/Los_Angeles"));
        assert_eq!(fp.platform, Platform::Linux);
        assert_eq!(fp.nav_platform(), "Linux x86_64");
        assert_eq!(fp.ch_platform(), "Linux");
        assert_eq!(fp.timezone, "America/Los_Angeles");
        assert!(WEBGL_RENDERERS_LINUX.contains(&fp.webgl_renderer.as_str()));
    }

    #[test]
    fn test_resolve_applies_timezone() {
        let fp = Fingerprint::resolve(None, Some("Asia/Tokyo"));
        assert_eq!(fp.timezone, "Asia/Tokyo");
    }

    #[test]
    fn test_resolve_ua_without_chrome_token_falls_back() {
        // A UA with no "Chrome/" token must still yield a plausible Chrome
        // full version via the fallback branch.
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Safari/537.36";
        let fp = Fingerprint::resolve(Some(ua), None);
        assert!(
            !fp.chrome_full_version.is_empty(),
            "fallback should populate chrome_full_version"
        );
        assert!(
            fp.chrome_full_version.starts_with(&fp.chrome_major),
            "fallback full version {} must start with major {}",
            fp.chrome_full_version,
            fp.chrome_major
        );
    }

    #[test]
    fn test_ch_brands_shape() {
        let fp = Fingerprint::resolve(
            Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.7312.86 Safari/537.36"),
            None,
        );
        let brands = fp.ch_brands();
        assert_eq!(brands.len(), 3);
        assert!(
            brands.contains(&("Google Chrome".to_string(), "140".to_string())),
            "ch_brands must include Google Chrome with major version: {:?}",
            brands
        );
        assert!(
            brands.iter().any(|(b, _)| b == "Not?A_Brand"),
            "ch_brands must include a GREASE Not?A_Brand entry: {:?}",
            brands
        );
    }

    #[test]
    fn test_ch_full_version_brands_shape() {
        let fp = Fingerprint::resolve(
            Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.7312.86 Safari/537.36"),
            None,
        );
        let brands = fp.ch_full_version_brands();
        assert_eq!(brands.len(), 3);
        assert!(
            brands.contains(&("Google Chrome".to_string(), "140.0.7312.86".to_string())),
            "ch_full_version_brands must include Google Chrome with full version: {:?}",
            brands
        );
        assert!(
            brands.iter().any(|(b, _)| b == "Not?A_Brand"),
            "ch_full_version_brands must include a GREASE Not?A_Brand entry: {:?}",
            brands
        );
    }
}
