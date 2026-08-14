//! Browser-side fetch helpers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Browser fetch request options.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFetchRequest {
    /// Request URL.
    pub url: String,
    /// HTTP method. Defaults to `GET`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Request headers.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Request body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Fetch credentials mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
    /// Fetch redirect mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect: Option<String>,
    /// Maximum response body characters to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_body_chars: Option<usize>,
}

impl BrowserFetchRequest {
    /// Create a GET request.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: None,
            headers: HashMap::new(),
            body: None,
            credentials: None,
            redirect: None,
            max_body_chars: None,
        }
    }
}

/// Browser fetch response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFetchResponse {
    /// Final response URL.
    pub url: String,
    /// HTTP status code.
    pub status: u16,
    /// HTTP status text.
    pub status_text: String,
    /// Whether the status is in the 2xx range.
    pub ok: bool,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body text, possibly truncated.
    pub body: String,
    /// Whether the body was truncated to `max_body_chars`.
    pub truncated: bool,
}

/// Per-request result for `fetch_many`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFetchOutcome {
    /// Request URL.
    pub url: String,
    /// Successful response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<BrowserFetchResponse>,
    /// Browser fetch error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(crate) fn fetch_script(request: &BrowserFetchRequest) -> crate::Result<String> {
    let request_json = serde_json::to_string(request)?;
    Ok(format!(
        r#"(async () => {{
  const request = {request_json};
  const maxBodyChars = request.maxBodyChars ?? null;
  const init = {{
    method: request.method ?? "GET",
    headers: request.headers ?? {{}},
    body: request.body ?? undefined,
    credentials: request.credentials ?? "same-origin",
    redirect: request.redirect ?? "follow"
  }};
  const response = await fetch(request.url, init);
  const headers = Object.fromEntries(response.headers.entries());
  const text = await response.text();
  const truncated = maxBodyChars !== null && text.length > maxBodyChars;
  return {{
    url: response.url,
    status: response.status,
    statusText: response.statusText,
    ok: response.ok,
    headers,
    body: truncated ? text.slice(0, maxBodyChars) : text,
    truncated
  }};
}})()"#
    ))
}

pub(crate) fn fetch_many_script(requests: &[BrowserFetchRequest]) -> crate::Result<String> {
    let requests_json = serde_json::to_string(requests)?;
    Ok(format!(
        r#"(async () => {{
  const requests = {requests_json};
  const run = async (request) => {{
    try {{
      const maxBodyChars = request.maxBodyChars ?? null;
      const response = await fetch(request.url, {{
        method: request.method ?? "GET",
        headers: request.headers ?? {{}},
        body: request.body ?? undefined,
        credentials: request.credentials ?? "same-origin",
        redirect: request.redirect ?? "follow"
      }});
      const text = await response.text();
      const truncated = maxBodyChars !== null && text.length > maxBodyChars;
      return {{
        url: request.url,
        response: {{
          url: response.url,
          status: response.status,
          statusText: response.statusText,
          ok: response.ok,
          headers: Object.fromEntries(response.headers.entries()),
          body: truncated ? text.slice(0, maxBodyChars) : text,
          truncated
        }}
      }};
    }} catch (error) {{
      return {{
        url: request.url,
        error: error instanceof Error ? error.message : String(error)
      }};
    }}
  }};
  return Promise.all(requests.map(run));
}})()"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_script_embeds_request_as_json() {
        let mut request = BrowserFetchRequest::get("https://example.com/?q='x'");
        request.headers.insert("x-test".into(), "1".into());
        request.max_body_chars = Some(10);
        let script = fetch_script(&request).unwrap();
        assert!(script.contains(r#""url":"https://example.com/?q='x'""#));
        assert!(script.contains("maxBodyChars"));
        assert!(script.contains("response.text()"));
    }

    #[test]
    fn fetch_many_script_uses_promise_all() {
        let script = fetch_many_script(&[BrowserFetchRequest::get("https://example.com")]).unwrap();
        assert!(script.contains("Promise.all"));
        assert!(script.contains("catch (error)"));
    }
}
