//! Page Abstraction
//!
//! High-level API for interacting with a browser page.

use std::collections::HashMap;
use std::sync::Arc;

use crate::cdp::{Session, Cookie, MouseEventType, MouseButton};
use crate::cdp::types::{NetworkRequest, NetworkResponse};
use crate::error::{Error, Result};
use crate::stealth::{Human, HumanSpeed};
use crate::StealthConfig;

/// A browser page with stealth capabilities
pub struct Page {
    session: Session,
    config: Arc<StealthConfig>,
}

impl Page {
    /// Create a new Page wrapping a CDP session
    pub(crate) fn new(session: Session, config: Arc<StealthConfig>) -> Self {
        Self { session, config }
    }

    /// Get the underlying CDP session
    pub fn session(&self) -> &Session {
        &self.session
    }

    // =========================================================================
    // Navigation
    // =========================================================================

    /// Navigate to a URL
    pub async fn goto(&self, url: &str) -> Result<()> {
        let result = self.session.navigate(url).await?;
        if let Some(error) = result.error_text {
            return Err(Error::Navigation(error));
        }
        // Wait for navigation to settle
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(())
    }

    /// Reload the page
    pub async fn reload(&self) -> Result<()> {
        self.session.reload(false).await
    }

    /// Go back in history
    pub async fn back(&self) -> Result<()> {
        self.session.go_back().await
    }

    /// Go forward in history
    pub async fn forward(&self) -> Result<()> {
        self.session.go_forward().await
    }

    /// Wait for navigation to complete by polling document.readyState
    ///
    /// Waits until the document is fully loaded (readyState === "complete").
    /// Times out after the specified duration (default: 30 seconds).
    pub async fn wait_for_navigation(&self) -> Result<()> {
        self.wait_for_navigation_timeout(30_000).await
    }

    /// Wait for navigation with a custom timeout in milliseconds
    pub async fn wait_for_navigation_timeout(&self, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        let poll_interval = std::time::Duration::from_millis(50);

        loop {
            // Check document.readyState
            match self.session.evaluate("document.readyState").await {
                Ok(result) => {
                    if let Some(value) = result.result.value {
                        if value.as_str() == Some("complete") {
                            return Ok(());
                        }
                    }
                }
                Err(_) => {
                    // Page might be navigating, readyState unavailable - keep waiting
                }
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Navigation did not complete within {}ms",
                    timeout_ms
                )));
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    // =========================================================================
    // Page Info
    // =========================================================================

    /// Get current URL
    pub async fn url(&self) -> Result<String> {
        let frame_tree = self.session.get_frame_tree().await?;
        Ok(frame_tree.frame.url)
    }

    /// Get page title
    pub async fn title(&self) -> Result<String> {
        let result = self.session.evaluate("document.title").await?;
        if let Some(value) = result.result.value {
            if let Some(s) = value.as_str() {
                return Ok(s.to_string());
            }
        }
        Ok(String::new())
    }

    /// Get page HTML content
    pub async fn content(&self) -> Result<String> {
        let result = self.session.evaluate("document.documentElement.outerHTML").await?;
        if let Some(value) = result.result.value {
            if let Some(s) = value.as_str() {
                return Ok(s.to_string());
            }
        }
        Ok(String::new())
    }

    /// Get page text content (body innerText)
    pub async fn text(&self) -> Result<String> {
        let result = self.session.evaluate("document.body.innerText").await?;
        if let Some(value) = result.result.value {
            if let Some(s) = value.as_str() {
                return Ok(s.to_string());
            }
        }
        Ok(String::new())
    }

    // =========================================================================
    // Screenshots
    // =========================================================================

    /// Capture a screenshot as PNG bytes
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        self.session.capture_screenshot(Some("png"), None).await
    }

    /// Capture a screenshot as JPEG with quality
    pub async fn screenshot_jpeg(&self, quality: u8) -> Result<Vec<u8>> {
        self.session.capture_screenshot(Some("jpeg"), Some(quality)).await
    }

    // =========================================================================
    // Element Finding
    // =========================================================================

    /// Find an element by CSS selector
    pub async fn find(&self, selector: &str) -> Result<Element<'_>> {
        let doc = self.session.get_document(Some(0)).await?;
        let node_id = self.session.query_selector(doc.node_id, selector).await?;

        if node_id == 0 {
            return Err(Error::ElementNotFound(selector.to_string()));
        }

        Ok(Element {
            page: self,
            node_id,
        })
    }

    /// Find all elements matching a CSS selector
    pub async fn find_all(&self, selector: &str) -> Result<Vec<Element<'_>>> {
        let doc = self.session.get_document(Some(0)).await?;
        let node_ids = self.session.query_selector_all(doc.node_id, selector).await?;

        Ok(node_ids
            .into_iter()
            .filter(|&id| id != 0)
            .map(|node_id| Element { page: self, node_id })
            .collect())
    }

    /// Check if an element exists
    pub async fn exists(&self, selector: &str) -> bool {
        self.find(selector).await.is_ok()
    }

    // =========================================================================
    // Interaction (Direct)
    // =========================================================================

    /// Click at coordinates
    pub async fn click_at(&self, x: f64, y: f64) -> Result<()> {
        // Mouse down
        self.session.dispatch_mouse_event(
            MouseEventType::MousePressed,
            x, y,
            Some(MouseButton::Left),
            Some(1),
        ).await?;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Mouse up
        self.session.dispatch_mouse_event(
            MouseEventType::MouseReleased,
            x, y,
            Some(MouseButton::Left),
            Some(1),
        ).await?;

        Ok(())
    }

    /// Click on an element by selector
    pub async fn click(&self, selector: &str) -> Result<()> {
        let element = self.find(selector).await?;
        element.click().await
    }

    /// Type text into focused element
    pub async fn type_text(&self, text: &str) -> Result<()> {
        self.session.insert_text(text).await
    }

    /// Type text into an element by selector
    pub async fn type_into(&self, selector: &str, text: &str) -> Result<()> {
        let element = self.find(selector).await?;
        element.click().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.session.insert_text(text).await
    }

    // =========================================================================
    // Human-like Interaction
    // =========================================================================

    /// Get a Human helper for human-like interactions
    pub fn human(&self) -> Human<'_> {
        Human::new(&self.session)
    }

    /// Human-like click on an element
    pub async fn human_click(&self, selector: &str) -> Result<()> {
        let element = self.find(selector).await?;
        let (x, y) = element.center().await?;

        if self.config.human_mouse {
            self.human().move_and_click(x, y).await
        } else {
            self.click_at(x, y).await
        }
    }

    /// Human-like click with speed option
    pub async fn human_click_with_speed(&self, selector: &str, speed: HumanSpeed) -> Result<()> {
        let element = self.find(selector).await?;
        let (x, y) = element.center().await?;

        self.human().with_speed(speed).move_and_click(x, y).await
    }

    /// Human-like typing into an element
    pub async fn human_type(&self, selector: &str, text: &str) -> Result<()> {
        // Click first
        self.human_click(selector).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        if self.config.human_typing {
            self.human().type_text(text).await
        } else {
            self.session.insert_text(text).await
        }
    }

    /// Human-like typing with speed option
    pub async fn human_type_with_speed(&self, selector: &str, text: &str, speed: HumanSpeed) -> Result<()> {
        self.human_click_with_speed(selector, speed).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.human().with_speed(speed).type_text(text).await
    }

    // =========================================================================
    // JavaScript Evaluation
    // =========================================================================

    /// Evaluate JavaScript and return the result
    pub async fn evaluate<T: serde::de::DeserializeOwned>(&self, expression: &str) -> Result<T> {
        let result = self.session.evaluate(expression).await?;

        if let Some(exception) = result.exception_details {
            return Err(Error::CdpSimple(format!(
                "JavaScript error: {} at {}:{}",
                exception.text, exception.line_number, exception.column_number
            )));
        }

        if let Some(value) = result.result.value {
            let typed: T = serde_json::from_value(value)?;
            return Ok(typed);
        }

        Err(Error::CdpSimple("No value returned from evaluate".into()))
    }

    /// Execute JavaScript without expecting a return value
    pub async fn execute(&self, expression: &str) -> Result<()> {
        let result = self.session.evaluate(expression).await?;

        if let Some(exception) = result.exception_details {
            return Err(Error::CdpSimple(format!(
                "JavaScript error: {} at {}:{}",
                exception.text, exception.line_number, exception.column_number
            )));
        }

        Ok(())
    }

    // =========================================================================
    // Cookies
    // =========================================================================

    /// Get all cookies
    pub async fn cookies(&self) -> Result<Vec<Cookie>> {
        self.session.get_cookies(None).await
    }

    /// Get cookies for specific URLs
    pub async fn cookies_for_urls(&self, urls: Vec<String>) -> Result<Vec<Cookie>> {
        self.session.get_cookies(Some(urls)).await
    }

    /// Set a cookie
    pub async fn set_cookie(
        &self,
        name: &str,
        value: &str,
        domain: Option<&str>,
        path: Option<&str>,
    ) -> Result<()> {
        let url = self.url().await.ok();
        let success = self.session.set_cookie(
            name,
            value,
            url.as_deref(),
            domain,
            path,
        ).await?;

        if !success {
            return Err(Error::CdpSimple("Failed to set cookie".into()));
        }
        Ok(())
    }

    /// Delete a cookie
    pub async fn delete_cookie(&self, name: &str, domain: Option<&str>) -> Result<()> {
        let url = self.url().await.ok();
        self.session.delete_cookies(name, url.as_deref(), domain).await
    }

    // =========================================================================
    // Wait Helpers
    // =========================================================================

    /// Wait for an element to appear
    pub async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<Element<'_>> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if let Ok(element) = self.find(selector).await {
                return Ok(element);
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Element '{}' not found within {}ms",
                    selector, timeout_ms
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait for an element to disappear
    pub async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if self.find(selector).await.is_err() {
                return Ok(());
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Element '{}' still visible after {}ms",
                    selector, timeout_ms
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait for a fixed duration
    pub async fn wait(&self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }

    // =========================================================================
    // Network Request Capture
    // =========================================================================

    /// Enable network request capture
    /// NOTE: This enables Network.enable which may be slightly detectable by advanced anti-bot
    pub async fn enable_request_capture(&self) -> Result<()> {
        self.session.network_enable().await
    }

    /// Disable network request capture
    pub async fn disable_request_capture(&self) -> Result<()> {
        self.session.network_disable().await
    }

    /// Get response body for a captured request
    /// The request_id comes from CapturedRequest.request_id
    pub async fn get_response_body(&self, request_id: &str) -> Result<ResponseBody> {
        let (body, base64_encoded) = self.session.get_response_body(request_id).await?;

        if base64_encoded {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&body)
                .map_err(|e| Error::Decode(e.to_string()))?;
            Ok(ResponseBody::Binary(bytes))
        } else {
            Ok(ResponseBody::Text(body))
        }
    }
}

/// A captured HTTP request with its response
#[derive(Debug, Clone)]
pub struct CapturedRequest {
    /// Request ID (use with get_response_body)
    pub request_id: String,
    /// Request URL
    pub url: String,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// POST data (if any)
    pub post_data: Option<String>,
    /// Resource type (Document, XHR, Fetch, etc.)
    pub resource_type: Option<String>,
    /// Response status code (if response received)
    pub status: Option<i32>,
    /// Response status text
    pub status_text: Option<String>,
    /// Response headers
    pub response_headers: Option<HashMap<String, String>>,
    /// Response MIME type
    pub mime_type: Option<String>,
    /// Request timestamp
    pub timestamp: f64,
    /// Whether the response is complete
    pub complete: bool,
}

impl CapturedRequest {
    /// Create from a request event
    pub fn from_request(
        request_id: String,
        request: &NetworkRequest,
        resource_type: Option<String>,
        timestamp: f64,
    ) -> Self {
        Self {
            request_id,
            url: request.url.clone(),
            method: request.method.clone(),
            headers: request.headers.clone(),
            post_data: request.post_data.clone(),
            resource_type,
            status: None,
            status_text: None,
            response_headers: None,
            mime_type: None,
            timestamp,
            complete: false,
        }
    }

    /// Update with response info
    pub fn set_response(&mut self, response: &NetworkResponse) {
        self.status = Some(response.status);
        self.status_text = Some(response.status_text.clone());
        self.response_headers = Some(response.headers.clone());
        self.mime_type = response.mime_type.clone();
    }

    /// Mark as complete
    pub fn mark_complete(&mut self) {
        self.complete = true;
    }
}

/// Response body - either text or binary
#[derive(Debug)]
pub enum ResponseBody {
    Text(String),
    Binary(Vec<u8>),
}

impl ResponseBody {
    /// Get as text (panics if binary)
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ResponseBody::Text(s) => Some(s),
            ResponseBody::Binary(_) => None,
        }
    }

    /// Get as bytes
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            ResponseBody::Text(s) => s.as_bytes(),
            ResponseBody::Binary(b) => b,
        }
    }

    /// Try to parse as JSON
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        match self {
            ResponseBody::Text(s) => Ok(serde_json::from_str(s)?),
            ResponseBody::Binary(b) => Ok(serde_json::from_slice(b)?),
        }
    }
}

/// An element on the page
pub struct Element<'a> {
    page: &'a Page,
    node_id: i32,
}

impl<'a> Element<'a> {
    /// Get the element's center coordinates
    pub async fn center(&self) -> Result<(f64, f64)> {
        let model = self.page.session.get_box_model(self.node_id).await?;
        Ok(model.center())
    }

    /// Click this element
    pub async fn click(&self) -> Result<()> {
        let (x, y) = self.center().await?;
        self.page.click_at(x, y).await
    }

    /// Human-like click
    pub async fn human_click(&self) -> Result<()> {
        let (x, y) = self.center().await?;
        self.page.human().move_and_click(x, y).await
    }

    /// Get outer HTML
    pub async fn outer_html(&self) -> Result<String> {
        self.page.session.get_outer_html(self.node_id).await
    }

    /// Get inner text (via JavaScript)
    pub async fn text(&self) -> Result<String> {
        // Focus the element first, then get its innerText
        self.page.session.focus(self.node_id).await?;
        let result = self.page.session.evaluate("document.activeElement.innerText").await?;
        if let Some(value) = result.result.value {
            if let Some(s) = value.as_str() {
                return Ok(s.to_string());
            }
        }
        Ok(String::new())
    }

    /// Type text into this element
    pub async fn type_text(&self, text: &str) -> Result<()> {
        self.click().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.page.session.insert_text(text).await
    }

    /// Focus this element
    pub async fn focus(&self) -> Result<()> {
        self.page.session.focus(self.node_id).await
    }
}
