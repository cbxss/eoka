//! Session export for browser-to-HTTP-client handoff
//!
//! After authenticating in the browser, export the session to make
//! direct HTTP requests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::cdp::Cookie;
use crate::error::Result;

/// Browser cookie (simplified, serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
    pub expires: Option<f64>,
}

impl From<Cookie> for SessionCookie {
    fn from(c: Cookie) -> Self {
        Self {
            name: c.name,
            value: c.value,
            domain: c.domain,
            path: c.path,
            secure: c.secure,
            http_only: c.http_only,
            same_site: c.same_site,
            expires: if c.expires > 0.0 {
                Some(c.expires)
            } else {
                None
            },
        }
    }
}

/// Exported browser session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSession {
    /// All cookies from the browser
    pub cookies: Vec<SessionCookie>,
    /// User agent string
    pub user_agent: String,
    /// Current URL
    pub url: String,
    /// Additional headers
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

impl BrowserSession {
    /// Create a new session from cookies
    pub fn new(cookies: Vec<Cookie>, user_agent: String, url: String) -> Self {
        Self {
            cookies: cookies.into_iter().map(SessionCookie::from).collect(),
            user_agent,
            url,
            extra_headers: HashMap::new(),
        }
    }

    /// Save session to JSON file
    pub fn save(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load session from JSON file
    pub fn load(path: &str) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let session = serde_json::from_str(&json)?;
        Ok(session)
    }

    /// Get cookies for a specific domain (RFC 6265 domain matching)
    pub fn cookies_for_domain(&self, domain: &str) -> Vec<&SessionCookie> {
        self.cookies
            .iter()
            .filter(|c| {
                let cookie_domain = c.domain.trim_start_matches('.');
                // Exact match or subdomain match with dot boundary
                domain == cookie_domain || domain.ends_with(&format!(".{}", cookie_domain))
            })
            .collect()
    }

    /// Format cookies as a Cookie header value (RFC 6265 compliant)
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|c| {
                // RFC 6265 §4.1.1: cookie-value must not contain semicolons,
                // commas, or whitespace. Percent-encode problematic chars.
                let safe_value = c
                    .value
                    .replace('%', "%25")
                    .replace(';', "%3B")
                    .replace(',', "%2C")
                    .replace(' ', "%20");
                format!("{}={}", c.name, safe_value)
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_header() {
        let session = BrowserSession {
            cookies: vec![
                SessionCookie {
                    name: "a".to_string(),
                    value: "1".to_string(),
                    domain: "example.com".to_string(),
                    path: "/".to_string(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                },
                SessionCookie {
                    name: "b".to_string(),
                    value: "2".to_string(),
                    domain: "example.com".to_string(),
                    path: "/".to_string(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                },
            ],
            user_agent: String::new(),
            url: String::new(),
            extra_headers: HashMap::new(),
        };

        let header = session.cookie_header();
        assert_eq!(header, "a=1; b=2");
    }

    #[test]
    fn test_cookies_for_domain() {
        let session = BrowserSession {
            cookies: vec![
                SessionCookie {
                    name: "site1".to_string(),
                    value: "v1".to_string(),
                    domain: "example.com".to_string(),
                    path: "/".to_string(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                },
                SessionCookie {
                    name: "site2".to_string(),
                    value: "v2".to_string(),
                    domain: "other.com".to_string(),
                    path: "/".to_string(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                },
                SessionCookie {
                    name: "sub".to_string(),
                    value: "v3".to_string(),
                    domain: ".example.com".to_string(),
                    path: "/".to_string(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                },
            ],
            user_agent: String::new(),
            url: String::new(),
            extra_headers: HashMap::new(),
        };

        // Exact match
        let example_cookies = session.cookies_for_domain("example.com");
        assert_eq!(example_cookies.len(), 2); // "example.com" + ".example.com"

        // Subdomain match
        let sub_cookies = session.cookies_for_domain("sub.example.com");
        assert_eq!(sub_cookies.len(), 2); // "example.com" + ".example.com"

        // Must NOT match "badexample.com" (no dot boundary)
        let bad_cookies = session.cookies_for_domain("badexample.com");
        assert_eq!(bad_cookies.len(), 0);

        // other.com only matches itself
        let other_cookies = session.cookies_for_domain("other.com");
        assert_eq!(other_cookies.len(), 1);
    }

    #[test]
    fn test_cookie_header_escaping() {
        let session = BrowserSession {
            cookies: vec![SessionCookie {
                name: "tok".to_string(),
                value: "val;ue with spaces".to_string(),
                domain: "x.com".to_string(),
                path: "/".to_string(),
                secure: false,
                http_only: false,
                same_site: None,
                expires: None,
            }],
            user_agent: String::new(),
            url: String::new(),
            extra_headers: HashMap::new(),
        };
        let header = session.cookie_header();
        assert!(!header.contains(';'));
        assert!(header.contains("tok=val%3Bue%20with%20spaces"));
    }
}
