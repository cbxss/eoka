//! Browser fingerprint generation
//!
//! Generates realistic, randomized browser fingerprints.

use rand::seq::SliceRandom;
use rand::Rng;

/// Chrome versions (recent, realistic)
const CHROME_VERSIONS: &[&str] = &[
    "120.0.0.0", "121.0.0.0", "122.0.0.0", "123.0.0.0", "124.0.0.0",
    "125.0.0.0", "126.0.0.0", "127.0.0.0", "128.0.0.0", "129.0.0.0",
    "130.0.0.0", "131.0.0.0", "132.0.0.0", "133.0.0.0", "134.0.0.0",
];

/// macOS versions
const MACOS_VERSIONS: &[&str] = &[
    "10_15_7", "11_0_0", "11_6_0", "12_0_0", "12_6_0",
    "13_0_0", "13_4_0", "14_0_0", "14_2_0", "14_4_0",
];

/// Windows versions
const WINDOWS_VERSIONS: &[&str] = &[
    "10.0", "10.0; Win64; x64",
];

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
    "ANGLE (NVIDIA, NVIDIA GeForce RTX 3080, Direct3D11)",
    "ANGLE (NVIDIA, NVIDIA GeForce RTX 4070, Direct3D11)",
    "ANGLE (NVIDIA, NVIDIA GeForce GTX 1080, Direct3D11)",
    "ANGLE (AMD, AMD Radeon RX 6800 XT, Direct3D11)",
    "ANGLE (Intel, Intel UHD Graphics 770, Direct3D11)",
];

/// Screen resolutions
const SCREEN_RESOLUTIONS: &[(u32, u32)] = &[
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
    (1440, 900),
    (1680, 1050),
    (2560, 1600),
    (3024, 1964),  // MacBook Pro 14"
    (3456, 2234),  // MacBook Pro 16"
];

/// Generate a random realistic user agent
pub fn random_user_agent() -> String {
    let mut rng = rand::thread_rng();

    let chrome_version = CHROME_VERSIONS.choose(&mut rng).unwrap();

    // 70% Mac, 30% Windows
    if rng.gen_bool(0.7) {
        let macos = MACOS_VERSIONS.choose(&mut rng).unwrap();
        format!(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X {}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36",
            macos, chrome_version
        )
    } else {
        let windows = WINDOWS_VERSIONS.choose(&mut rng).unwrap();
        format!(
            "Mozilla/5.0 (Windows NT {}; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36",
            windows, chrome_version
        )
    }
}

/// Browser fingerprint data
#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub user_agent: String,
    pub platform: Platform,
    pub screen_width: u32,
    pub screen_height: u32,
    pub color_depth: u8,
    pub hardware_concurrency: u8,
    pub device_memory: u8,
    pub timezone: String,
    pub languages: Vec<String>,
    pub webgl_vendor: String,
    pub webgl_renderer: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Platform {
    MacOS,
    Windows,
}

impl Fingerprint {
    /// Generate a random consistent fingerprint
    pub fn random() -> Self {
        let mut rng = rand::thread_rng();

        let platform = if rng.gen_bool(0.7) {
            Platform::MacOS
        } else {
            Platform::Windows
        };

        let (screen_width, screen_height) = *SCREEN_RESOLUTIONS.choose(&mut rng).unwrap();

        let (webgl_vendor, webgl_renderer) = match platform {
            Platform::MacOS => {
                let renderer = WEBGL_RENDERERS_MAC.choose(&mut rng).unwrap();
                ("Google Inc. (Apple)", *renderer)
            }
            Platform::Windows => {
                let renderer = WEBGL_RENDERERS_WINDOWS.choose(&mut rng).unwrap();
                ("Google Inc. (NVIDIA Corporation)", *renderer)
            }
        };

        let hardware_concurrency = *[4, 8, 10, 12, 16].choose(&mut rng).unwrap();
        let device_memory = *[8, 16, 32].choose(&mut rng).unwrap();

        Self {
            user_agent: random_user_agent(),
            platform,
            screen_width,
            screen_height,
            color_depth: 24,
            hardware_concurrency,
            device_memory,
            timezone: "America/Los_Angeles".to_string(),
            languages: vec!["en-US".to_string(), "en".to_string()],
            webgl_vendor: webgl_vendor.to_string(),
            webgl_renderer: webgl_renderer.to_string(),
        }
    }

    /// Generate JavaScript to apply this fingerprint
    pub fn to_evasion_script(&self) -> String {
        format!(
            r#"
Object.defineProperty(navigator, 'hardwareConcurrency', {{ get: () => {hw} }});
Object.defineProperty(navigator, 'deviceMemory', {{ get: () => {mem} }});
Object.defineProperty(screen, 'width', {{ get: () => {sw} }});
Object.defineProperty(screen, 'height', {{ get: () => {sh} }});
Object.defineProperty(screen, 'colorDepth', {{ get: () => {cd} }});
"#,
            hw = self.hardware_concurrency,
            mem = self.device_memory,
            sw = self.screen_width,
            sh = self.screen_height,
            cd = self.color_depth,
        )
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
    fn test_fingerprint_random() {
        let fp = Fingerprint::random();
        assert!(!fp.user_agent.is_empty());
        assert!(fp.screen_width > 0);
        assert!(fp.screen_height > 0);
        assert!([4, 8, 10, 12, 16].contains(&fp.hardware_concurrency));
        assert!([8, 16, 32].contains(&fp.device_memory));
    }
}
