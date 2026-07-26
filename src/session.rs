//! Session export for browser-to-HTTP-client handoff
//!
//! After authenticating in the browser, export the session to make
//! direct HTTP requests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;

/// Browser cookie (simplified, serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
    /// Domain the cookie applies to.
    pub domain: String,
    /// Path the cookie applies to.
    pub path: String,
    /// Whether the cookie is sent only over HTTPS.
    pub secure: bool,
    /// Whether the cookie is inaccessible to JavaScript.
    pub http_only: bool,
    /// SameSite policy ("Strict", "Lax", "None"), if set.
    pub same_site: Option<String>,
    /// Expiry as a Unix timestamp in seconds, or `None` for a session cookie.
    pub expires: Option<f64>,
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

/// Complete browser state for restoring an authenticated browser session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    /// All browser cookies, including HttpOnly cookies.
    pub cookies: Vec<SessionCookie>,
    /// Storage associated with the saved URL's origin.
    #[serde(default)]
    pub local_storage: HashMap<String, String>,
    /// Per-tab storage associated with the saved URL's origin.
    #[serde(default)]
    pub session_storage: HashMap<String, String>,
    /// User agent active when the state was captured.
    pub user_agent: String,
    /// URL whose origin owns the captured storage.
    pub url: String,
}

impl BrowserSession {
    /// Create a new session from cookies
    pub fn new(cookies: Vec<SessionCookie>, user_agent: String, url: String) -> Self {
        Self {
            cookies,
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
                if let Some(base) = c.domain.strip_prefix('.') {
                    // Domain cookie: matches the domain and its subdomains.
                    domain == base || domain.ends_with(&format!(".{}", base))
                } else {
                    // Host-only cookie: matches only the exact host.
                    domain == c.domain
                }
            })
            .collect()
    }

    /// Format cookies as a Cookie header value (RFC 6265 compliant)
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|c| {
                format!(
                    "{}={}",
                    Self::sanitize_cookie_name(&c.name),
                    Self::escape_cookie_value(&c.value)
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Percent-encode characters illegal in an RFC 6265 cookie-value.
    fn escape_cookie_value(value: &str) -> String {
        value
            .replace('%', "%25")
            .replace(';', "%3B")
            .replace(',', "%2C")
            .replace(' ', "%20")
            .replace('\r', "%0D")
            .replace('\n', "%0A")
    }

    /// Percent-encode any byte not valid in an RFC 6265 cookie-name.
    fn sanitize_cookie_name(name: &str) -> String {
        let mut out = String::with_capacity(name.len());
        for b in name.bytes() {
            let is_separator = matches!(
                b,
                b'(' | b')'
                    | b'<'
                    | b'>'
                    | b'@'
                    | b','
                    | b';'
                    | b':'
                    | b'\\'
                    | b'"'
                    | b'/'
                    | b'['
                    | b']'
                    | b'?'
                    | b'='
                    | b'{'
                    | b'}'
            );
            if b > 0x20 && b < 0x7f && !is_separator {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
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
    fn test_browser_state_round_trips_storage_and_http_only_cookie() {
        let state = BrowserState {
            cookies: vec![SessionCookie {
                name: "session".to_string(),
                value: "secret".to_string(),
                domain: ".example.com".to_string(),
                path: "/".to_string(),
                secure: true,
                http_only: true,
                same_site: Some("Lax".to_string()),
                expires: None,
            }],
            local_storage: HashMap::from([("token".to_string(), "value".to_string())]),
            session_storage: HashMap::from([("tab".to_string(), "value".to_string())]),
            user_agent: "Mozilla/5.0".to_string(),
            url: "https://example.com/account".to_string(),
        };

        let json = serde_json::to_string(&state).unwrap();
        let restored: BrowserState = serde_json::from_str(&json).unwrap();

        assert!(restored.cookies[0].http_only);
        assert_eq!(restored.local_storage["token"], "value");
        assert_eq!(restored.session_storage["tab"], "value");
        assert_eq!(restored.url, "https://example.com/account");
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

        // Exact host match
        let example_cookies = session.cookies_for_domain("example.com");
        assert_eq!(example_cookies.len(), 2); // "example.com" + ".example.com"

        // Subdomain: only the domain cookie ".example.com" matches
        let sub_cookies = session.cookies_for_domain("sub.example.com");
        assert_eq!(sub_cookies.len(), 1); // only ".example.com"

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

    /// Build a single-cookie session for header-encoding tests.
    fn single_cookie(name: &str, value: &str) -> BrowserSession {
        BrowserSession {
            cookies: vec![SessionCookie {
                name: name.to_string(),
                value: value.to_string(),
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
        }
    }

    #[test]
    fn test_cookie_header_no_crlf_injection() {
        // A value carrying a CRLF header-injection payload must be neutralized:
        // no raw CR or LF may survive into the header string.
        let session = single_cookie("sid", "x\r\nX-Injected: 1");
        let header = session.cookie_header();
        assert!(!header.contains('\r'), "raw CR leaked: {:?}", header);
        assert!(!header.contains('\n'), "raw LF leaked: {:?}", header);
        // CR and LF are percent-encoded.
        assert!(header.contains("%0D"));
        assert!(header.contains("%0A"));
        assert_eq!(header, "sid=x%0D%0AX-Injected:%201");
    }

    #[test]
    fn test_cookie_header_value_special_chars_encoded() {
        // ';', ',', space and '%' are all percent-encoded.
        let session = single_cookie("k", "a;b,c d");
        let header = session.cookie_header();
        assert_eq!(header, "k=a%3Bb%2Cc%20d");
        assert!(!header.contains(';'));
        assert!(!header.contains(','));
        assert!(!header.contains(' '));
    }

    #[test]
    fn test_cookie_header_percent_first_no_double_encode() {
        // '%' must be escaped first so a literal '%' becomes exactly %25 and
        // existing percent sequences in the value are not re-interpreted.
        let session = single_cookie("k", "100%");
        let header = session.cookie_header();
        assert_eq!(header, "k=100%25");

        // A literal "%3B" in the value must not be mistaken for an encoded ';'.
        let session = single_cookie("k", "%3B");
        let header = session.cookie_header();
        assert_eq!(header, "k=%253B");
    }

    #[test]
    fn test_cookie_header_name_sanitized() {
        // Control chars, spaces and separators in the NAME must be
        // percent-encoded so they cannot appear unescaped in the header.
        let session = single_cookie("a b;c=d\ne", "1");
        let header = session.cookie_header();
        // The value part is trivially "1"; inspect the name portion.
        let name = header.strip_suffix("=1").expect("header ends with =1");
        assert!(!name.contains(' '), "raw space in name: {:?}", name);
        assert!(!name.contains(';'), "raw ; in name: {:?}", name);
        assert!(!name.contains('\n'), "raw LF in name: {:?}", name);
        // The '=' that is part of the cookie name must be escaped (%3D), so
        // there is exactly one unescaped '=' separator in the whole header.
        assert_eq!(header.matches('=').count(), 1);
        assert_eq!(name, "a%20b%3Bc%3Dd%0Ae");
    }

    #[test]
    fn test_cookies_for_domain_host_only_vs_domain() {
        let session = BrowserSession {
            cookies: vec![
                // Host-only cookie: stored domain has no leading dot.
                SessionCookie {
                    name: "host".to_string(),
                    value: "1".to_string(),
                    domain: "example.com".to_string(),
                    path: "/".to_string(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                },
                // Domain cookie: stored domain has a leading dot.
                SessionCookie {
                    name: "dom".to_string(),
                    value: "2".to_string(),
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

        let names = |domain: &str| -> Vec<String> {
            session
                .cookies_for_domain(domain)
                .iter()
                .map(|c| c.name.clone())
                .collect()
        };

        // Exact host: both host-only and domain cookie match.
        let exact = names("example.com");
        assert!(exact.contains(&"host".to_string()));
        assert!(exact.contains(&"dom".to_string()));
        assert_eq!(exact.len(), 2);

        // Subdomain: only the domain cookie matches (host-only does NOT).
        let sub = names("sub.example.com");
        assert_eq!(sub, vec!["dom".to_string()]);

        // Suffix that is not a dot-boundary subdomain matches neither.
        let bad = names("badexample.com");
        assert!(bad.is_empty());
    }
}
