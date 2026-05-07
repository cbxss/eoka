//! HTTP discovery for Chrome's DevTools endpoint.
//!
//! Chrome started with `--remote-debugging-port=N` exposes a small HTTP API:
//! - `GET /json/version` — browser-level info, including `webSocketDebuggerUrl`
//! - `GET /json` — list of page targets, each with its own `webSocketDebuggerUrl`
//!
//! Chrome rejects requests whose `Host` header isn't `localhost`/`127.0.0.1` as
//! anti-DNS-rebinding. We always send `Host: localhost` to stay compatible.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// One entry from `GET /json` — a single browsing target (tab, iframe, worker).
#[derive(Debug, Clone, Deserialize)]
pub struct PageTarget {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl", default)]
    pub web_socket_debugger_url: String,
}

/// Resolve the browser-level DevTools WebSocket URL for a running Chrome.
///
/// ```no_run
/// let url = eoka::cdp::discover::discover_browser_ws("127.0.0.1", 9222)?;
/// # Ok::<(), eoka::Error>(())
/// ```
pub fn discover_browser_ws(host: &str, port: u16) -> Result<String> {
    let body = http_get(host, port, "/json/version")?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| Error::transport(format!("Invalid JSON from /json/version: {}", e)))?;
    value
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| Error::transport("No webSocketDebuggerUrl in /json/version"))
}

/// List all current targets (tabs, workers, iframes) for a running Chrome.
pub fn discover_pages(host: &str, port: u16) -> Result<Vec<PageTarget>> {
    let body = http_get(host, port, "/json")?;
    serde_json::from_str(&body)
        .map_err(|e| Error::transport(format!("Invalid JSON from /json: {}", e)))
}

/// Probe `start..=end` on `127.0.0.1` and return the first port whose Chrome
/// answers `/json/version`, alongside its WebSocket URL.
pub fn auto_connect(start: u16, end: u16) -> Option<(u16, String)> {
    (start..=end).find_map(|port| {
        discover_browser_ws("127.0.0.1", port)
            .ok()
            .map(|u| (port, u))
    })
}

/// Minimal blocking HTTP/1.1 GET. Reads until the server closes the connection.
fn http_get(host: &str, port: u16, path: &str) -> Result<String> {
    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| Error::transport_io(format!("Failed to connect to {}", addr), e))?;
    stream
        .set_read_timeout(Some(HTTP_TIMEOUT))
        .map_err(|e| Error::transport_io("set_read_timeout", e))?;
    stream
        .set_write_timeout(Some(HTTP_TIMEOUT))
        .map_err(|e| Error::transport_io("set_write_timeout", e))?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        path
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| Error::transport_io("HTTP write", e))?;

    let mut buf = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut buf)
        .map_err(|e| Error::transport_io("HTTP read", e))?;

    let raw = String::from_utf8_lossy(&buf);
    let header_end = raw
        .find("\r\n\r\n")
        .ok_or_else(|| Error::transport("Malformed HTTP response from Chrome"))?;
    let status_line = raw.lines().next().unwrap_or("");
    if !(status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200")) {
        return Err(Error::transport(format!(
            "HTTP {} from Chrome at {}: {}",
            path, addr, status_line
        )));
    }
    Ok(raw[header_end + 4..].to_string())
}
