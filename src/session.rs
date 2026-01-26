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
            expires: if c.expires > 0.0 { Some(c.expires) } else { None },
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

    /// Get cookies for a specific domain
    pub fn cookies_for_domain(&self, domain: &str) -> Vec<&SessionCookie> {
        self.cookies
            .iter()
            .filter(|c| domain.ends_with(&c.domain) || c.domain.ends_with(domain))
            .collect()
    }

    /// Format cookies as a Cookie header value
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Format cookies for a specific domain as a Cookie header
    pub fn cookie_header_for_domain(&self, domain: &str) -> String {
        self.cookies_for_domain(domain)
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Add an extra header
    pub fn add_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.extra_headers.insert(name.into(), value.into());
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
            ],
            user_agent: String::new(),
            url: String::new(),
            extra_headers: HashMap::new(),
        };

        let example_cookies = session.cookies_for_domain("example.com");
        assert_eq!(example_cookies.len(), 1);
        assert_eq!(example_cookies[0].name, "site1");
    }
}
