//! Page Abstraction
//!
//! High-level API for interacting with a browser page.

use std::collections::HashMap;
use std::sync::Arc;

use crate::cdp::{Cookie, MouseButton, MouseEventType, Session};
use crate::error::{Error, Result};
use crate::stealth::Human;
use crate::StealthConfig;

/// Escape a string for safe use in JavaScript string literals
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace("${", "\\${")
}

/// Check if a CDP error is an element-related error (not found, not visible, etc.)
fn is_element_cdp_error(e: &Error) -> bool {
    match e {
        Error::ElementNotFound(_) | Error::ElementNotVisible { .. } => true,
        Error::Cdp { message, .. } => {
            message.contains("box model")
                || message.contains("Could not find node")
                || message.contains("No node with given id")
                || message.contains("Node is not an element")
        }
        _ => false,
    }
}
/// Text matching strategy for find_by_text operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextMatch {
    /// Exact match (trimmed, case-sensitive)
    Exact,
    /// Contains the text (case-insensitive) - default
    #[default]
    Contains,
    /// Starts with the text (case-insensitive)
    StartsWith,
    /// Ends with the text (case-insensitive)
    EndsWith,
}

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

    /// Navigate to a URL
    pub async fn goto(&self, url: &str) -> Result<()> {
        let result = self.session.navigate(url).await?;
        if let Some(error) = result.error_text {
            return Err(Error::Navigation(error));
        }
        // Brief settle time for navigation to start. Use wait_for_navigation()
        // for reliable completion detection.
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
    /// Get current URL
    pub async fn url(&self) -> Result<String> {
        let frame_tree = self.session.get_frame_tree().await?;
        Ok(frame_tree.frame.url)
    }

    /// Get page title
    pub async fn title(&self) -> Result<String> {
        self.evaluate("document.title || ''").await
    }

    /// Get page HTML content
    pub async fn content(&self) -> Result<String> {
        self.evaluate("document.documentElement.outerHTML").await
    }

    /// Get page text content (body innerText)
    pub async fn text(&self) -> Result<String> {
        self.evaluate("document.body?.innerText || ''").await
    }
    /// Capture a screenshot as PNG bytes
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        self.session.capture_screenshot(Some("png"), None).await
    }

    /// Capture a screenshot as JPEG with quality
    pub async fn screenshot_jpeg(&self, quality: u8) -> Result<Vec<u8>> {
        self.session
            .capture_screenshot(Some("jpeg"), Some(quality))
            .await
    }
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
        let node_ids = self
            .session
            .query_selector_all(doc.node_id, selector)
            .await?;

        Ok(node_ids
            .into_iter()
            .filter(|&id| id != 0)
            .map(|node_id| Element {
                page: self,
                node_id,
            })
            .collect())
    }

    /// Check if an element exists
    #[must_use = "returns true if element exists"]
    pub async fn exists(&self, selector: &str) -> bool {
        self.find(selector).await.is_ok()
    }
    /// Find an element by its text content (case-insensitive contains)
    pub async fn find_by_text(&self, text: &str) -> Result<Element<'_>> {
        self.find_by_text_match(text, TextMatch::Contains).await
    }

    /// Find an element by text with specific matching strategy
    ///
    /// Prioritizes interactive elements (a, button, input) over static elements.
    /// Uses Runtime.callFunctionOn to avoid mutating the DOM (no marker attributes).
    pub async fn find_by_text_match(
        &self,
        text: &str,
        match_type: TextMatch,
    ) -> Result<Element<'_>> {
        let escaped_text = escape_js_string(text);
        let match_js = match match_type {
            TextMatch::Exact => format!("t.trim() === '{}'", escaped_text),
            TextMatch::Contains => format!(
                "t.toLowerCase().includes('{}')",
                escaped_text.to_lowercase()
            ),
            TextMatch::StartsWith => format!(
                "t.toLowerCase().startsWith('{}')",
                escaped_text.to_lowercase()
            ),
            TextMatch::EndsWith => format!(
                "t.toLowerCase().endsWith('{}')",
                escaped_text.to_lowercase()
            ),
        };

        // Find the element and return its node via DOM.querySelector with a unique class?
        // Better: evaluate JS that returns the element as a remote object, then get its node_id.
        // We use Runtime.evaluate with returnByValue=false to get an object reference,
        // then DOM.requestNode to get the node_id.
        let js = format!(
            r#"
            (() => {{
                const interactive = 'a, button, input[type="submit"], input[type="button"], [role="button"], [onclick]';
                for (const el of document.querySelectorAll(interactive)) {{
                    const t = el.innerText || el.textContent || el.value || '';
                    if ({match_js}) return el;
                }}
                const secondary = 'label, span, div, p, h1, h2, h3, h4, h5, h6, li, td, th';
                for (const el of document.querySelectorAll(secondary)) {{
                    const t = el.innerText || el.textContent || el.value || '';
                    if ({match_js}) return el;
                }}
                return null;
            }})()
            "#,
            match_js = match_js
        );

        let result = self.session.evaluate_for_remote_object(&js).await?;
        let remote = self.check_js_result(result)?;

        if remote.subtype.as_deref() == Some("null") {
            return Err(Error::ElementNotFound(format!("text: {}", text)));
        }

        let object_id = remote
            .object_id
            .ok_or_else(|| Error::ElementNotFound(format!("text: {}", text)))?;

        // Convert remote object to DOM node_id
        let node_id = self.session.request_node(&object_id).await?;

        if node_id == 0 {
            return Err(Error::ElementNotFound(format!("text: {}", text)));
        }

        Ok(Element {
            page: self,
            node_id,
        })
    }

    /// Find all elements matching the given text
    pub async fn find_all_by_text(&self, text: &str) -> Result<Vec<Element<'_>>> {
        let escaped_text = escape_js_string(text).to_lowercase();

        let js = format!(
            r#"
            (() => {{
                const selectors = 'a, button, input, label, span, div, p, h1, h2, h3, h4, h5, h6, li, td, th';
                const elements = document.querySelectorAll(selectors);
                const matches = [];
                for (const el of elements) {{
                    const t = (el.innerText || el.textContent || el.value || '').toLowerCase();
                    if (t.includes('{escaped_text}')) {{
                        matches.push(el);
                    }}
                }}
                return matches;
            }})()
            "#,
            escaped_text = escaped_text
        );

        // Evaluate without returnByValue to get remote object references
        let result = self.session.evaluate_for_remote_object(&js).await?;

        let remote = self.check_js_result(result)?;

        let array_object_id = match &remote.object_id {
            Some(id) => id.clone(),
            None => return Ok(Vec::new()),
        };

        // Get all indexed properties of the array in one CDP call
        let properties = self.session.get_properties(&array_object_id).await?;

        let mut elements = Vec::new();
        for prop in &properties {
            // Array elements have numeric names; skip "length" and prototype props
            if prop.name.parse::<usize>().is_err() {
                continue;
            }
            if let Some(ref obj_id) = prop.value.as_ref().and_then(|v| v.object_id.clone()) {
                if let Ok(node_id) = self.session.request_node(obj_id).await {
                    if node_id != 0 {
                        elements.push(Element {
                            page: self,
                            node_id,
                        });
                    }
                }
            }
        }

        Ok(elements)
    }

    /// Check if an element with the given text exists
    #[must_use = "returns true if text exists on page"]
    pub async fn text_exists(&self, text: &str) -> bool {
        self.find_by_text(text).await.is_ok()
    }
    /// Click at coordinates
    pub async fn click_at(&self, x: f64, y: f64) -> Result<()> {
        // Mouse down
        self.session
            .dispatch_mouse_event(
                MouseEventType::MousePressed,
                x,
                y,
                Some(MouseButton::Left),
                Some(1),
            )
            .await?;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Mouse up
        self.session
            .dispatch_mouse_event(
                MouseEventType::MouseReleased,
                x,
                y,
                Some(MouseButton::Left),
                Some(1),
            )
            .await?;

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

    /// Click an element by its text content
    pub async fn click_by_text(&self, text: &str) -> Result<()> {
        let element = self.find_by_text(text).await?;
        element.click().await
    }

    /// Try to click an element, returning Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_click(&self, selector: &str) -> Result<bool> {
        match self.find(selector).await {
            Ok(element) => match element.click().await {
                Ok(()) => Ok(true),
                Err(e) if is_element_cdp_error(&e) => Ok(false),
                Err(e) => Err(e),
            },
            Err(e) if is_element_cdp_error(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Try to click an element by text, returning Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_click_by_text(&self, text: &str) -> Result<bool> {
        match self.find_by_text(text).await {
            Ok(element) => match element.click().await {
                Ok(()) => Ok(true),
                Err(e) if is_element_cdp_error(&e) => Ok(false),
                Err(e) => Err(e),
            },
            Err(e) if is_element_cdp_error(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Fill a form field: click, clear, type
    pub async fn fill(&self, selector: &str, value: &str) -> Result<()> {
        let element = self.find(selector).await?;
        element.click().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Focus + select via selector (don't rely on activeElement — popups can steal focus)
        let escaped = escape_js_string(selector);
        self.execute(&format!(
            "(() => {{ const el = document.querySelector('{}'); if (el) {{ el.focus(); el.select(); }} }})()",
            escaped
        )).await?;
        self.session.insert_text("").await?;

        // Now type the new value
        self.session.insert_text(value).await
    }
    /// Get a Human helper for human-like interactions
    pub fn human(&self) -> Human<'_> {
        Human::new(&self.session)
    }

    /// Human-like click on an element
    pub async fn human_click(&self, selector: &str) -> Result<()> {
        let element = self.find(selector).await?;
        let (x, y) = element.center().await?;
        self.human_click_at_center_xy(x, y).await
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

    /// Human-like click on an element found by text content
    pub async fn human_click_by_text(&self, text: &str) -> Result<()> {
        let element = self.find_by_text(text).await?;
        let (x, y) = element.center().await?;
        self.human_click_at_center_xy(x, y).await
    }

    /// Try to human-click an element, returning Ok(true) if clicked, Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_human_click(&self, selector: &str) -> Result<bool> {
        self.try_human_click_impl(self.find(selector).await).await
    }

    /// Try to human-click an element by text, returning Ok(true) if clicked, Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_human_click_by_text(&self, text: &str) -> Result<bool> {
        self.try_human_click_impl(self.find_by_text(text).await)
            .await
    }

    /// Human-like form fill: click, clear, type with natural delays
    pub async fn human_fill(&self, selector: &str, value: &str) -> Result<()> {
        // Human click on the field
        self.human_click(selector).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Select all to clear (Cmd+A / Ctrl+A behavior via select())
        self.execute("document.activeElement.select()").await?;

        // Small pause before typing
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Type with human-like delays
        if self.config.human_typing {
            self.human().type_text(value).await
        } else {
            self.session.insert_text(value).await
        }
    }

    async fn human_click_at_center_xy(&self, x: f64, y: f64) -> Result<()> {
        if self.config.human_mouse {
            self.human().move_and_click(x, y).await
        } else {
            self.click_at(x, y).await
        }
    }

    /// Shared impl for try_human_click and try_human_click_by_text
    async fn try_human_click_impl(&self, find_result: Result<Element<'_>>) -> Result<bool> {
        match find_result {
            Ok(element) => match element.center().await {
                Ok((x, y)) => {
                    self.human_click_at_center_xy(x, y).await?;
                    Ok(true)
                }
                Err(e) if is_element_cdp_error(&e) => Ok(false),
                Err(e) => Err(e),
            },
            Err(e) if is_element_cdp_error(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }
    /// Evaluate JavaScript and return the result
    pub async fn evaluate<T: serde::de::DeserializeOwned>(&self, expression: &str) -> Result<T> {
        let result = self.check_js_result(self.session.evaluate(expression).await?)?;
        let value = result
            .value
            .ok_or_else(|| Error::CdpSimple("No value returned from evaluate".into()))?;
        Ok(serde_json::from_value(value)?)
    }

    /// Evaluate JavaScript synchronously (don't await promises).
    /// Use when the page may have unresolved promises that block normal evaluate.
    pub async fn evaluate_sync<T: serde::de::DeserializeOwned>(
        &self,
        expression: &str,
    ) -> Result<T> {
        let result = self.check_js_result(self.session.evaluate_sync(expression).await?)?;
        let value = result
            .value
            .ok_or_else(|| Error::CdpSimple("No value returned from evaluate_sync".into()))?;
        Ok(serde_json::from_value(value)?)
    }

    /// Execute JavaScript without expecting a return value
    pub async fn execute(&self, expression: &str) -> Result<()> {
        self.check_js_result(self.session.evaluate(expression).await?)?;
        Ok(())
    }

    /// Execute JavaScript synchronously (don't await promises)
    pub async fn execute_sync(&self, expression: &str) -> Result<()> {
        self.check_js_result(self.session.evaluate_sync(expression).await?)?;
        Ok(())
    }

    /// Check a JS evaluation result for exceptions
    fn check_js_result(
        &self,
        result: crate::cdp::types::RuntimeEvaluateResult,
    ) -> Result<crate::cdp::types::RemoteObject> {
        if let Some(exception) = result.exception_details {
            return Err(Error::CdpSimple(format!(
                "JavaScript error: {} at {}:{}",
                exception.text, exception.line_number, exception.column_number
            )));
        }
        Ok(result.result)
    }
    /// Get all cookies
    pub async fn cookies(&self) -> Result<Vec<Cookie>> {
        self.session.get_cookies(None).await
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
        let success = self
            .session
            .set_cookie(name, value, url.as_deref(), domain, path)
            .await?;

        if !success {
            return Err(Error::CdpSimple("Failed to set cookie".into()));
        }
        Ok(())
    }

    /// Delete a cookie
    pub async fn delete_cookie(&self, name: &str, domain: Option<&str>) -> Result<()> {
        let url = self.url().await.ok();
        self.session
            .delete_cookies(name, url.as_deref(), domain)
            .await
    }
    /// Wait for an element to appear in the DOM
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

    /// Wait for an element to be visible and clickable
    pub async fn wait_for_visible(&self, selector: &str, timeout_ms: u64) -> Result<Element<'_>> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if let Ok(element) = self.find(selector).await {
                // Check if we can compute box model (element is visible/rendered)
                if element.center().await.is_ok() {
                    return Ok(element);
                }
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Element '{}' not visible within {}ms",
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

    /// Wait for an element with specific text to appear
    pub async fn wait_for_text(&self, text: &str, timeout_ms: u64) -> Result<Element<'_>> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if let Ok(element) = self.find_by_text(text).await {
                return Ok(element);
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Element with text '{}' not found within {}ms",
                    text, timeout_ms
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait for the URL to contain a specific string
    pub async fn wait_for_url_contains(&self, pattern: &str, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if let Ok(url) = self.url().await {
                if url.contains(pattern) {
                    return Ok(());
                }
            }

            if start.elapsed() > timeout {
                let current_url = self.url().await.unwrap_or_else(|_| "unknown".to_string());
                return Err(Error::Timeout(format!(
                    "URL did not contain '{}' within {}ms (current: {})",
                    pattern, timeout_ms, current_url
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait for URL to change from current URL
    pub async fn wait_for_url_change(&self, timeout_ms: u64) -> Result<String> {
        let original_url = self.url().await?;
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if let Ok(url) = self.url().await {
                if url != original_url {
                    return Ok(url);
                }
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "URL did not change from '{}' within {}ms",
                    original_url, timeout_ms
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
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
    /// Find the first element matching any of the given selectors
    pub async fn find_any(&self, selectors: &[&str]) -> Result<Element<'_>> {
        for selector in selectors {
            if let Ok(element) = self.find(selector).await {
                return Ok(element);
            }
        }
        Err(Error::ElementNotFound(format!(
            "None of selectors found: {:?}",
            selectors
        )))
    }

    /// Wait for any of the given selectors to appear
    ///
    /// Returns the first selector that matches.
    pub async fn wait_for_any(&self, selectors: &[&str], timeout_ms: u64) -> Result<Element<'_>> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            for selector in selectors {
                if let Ok(element) = self.find(selector).await {
                    return Ok(element);
                }
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "None of selectors found within {}ms: {:?}",
                    timeout_ms, selectors
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    /// Wait for network to become idle (no pending XHR/fetch for `idle_time_ms`)
    pub async fn wait_for_network_idle(&self, idle_time_ms: u64, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        let idle_duration = std::time::Duration::from_millis(idle_time_ms);

        // Use JavaScript to monitor network activity
        let check_idle_js = r#"
            (() => {
                // Check if there are pending fetches/XHRs
                if (window.__eoka_pending_requests === undefined) {
                    window.__eoka_pending_requests = 0;

                    // Intercept fetch
                    const originalFetch = window.fetch;
                    window.fetch = function(...args) {
                        window.__eoka_pending_requests++;
                        return originalFetch.apply(this, args).finally(() => {
                            window.__eoka_pending_requests--;
                        });
                    };

                    // Intercept XHR
                    const originalOpen = XMLHttpRequest.prototype.open;
                    const originalSend = XMLHttpRequest.prototype.send;
                    XMLHttpRequest.prototype.open = function(...args) {
                        this.__eoka_tracked = true;
                        return originalOpen.apply(this, args);
                    };
                    XMLHttpRequest.prototype.send = function(...args) {
                        if (this.__eoka_tracked) {
                            window.__eoka_pending_requests++;
                            this.addEventListener('loadend', () => {
                                window.__eoka_pending_requests--;
                            });
                        }
                        return originalSend.apply(this, args);
                    };
                }
                return window.__eoka_pending_requests;
            })()
        "#;

        // Install the interceptors
        let _: i32 = self.evaluate(check_idle_js).await.unwrap_or(0);

        let mut idle_start: Option<std::time::Instant> = None;

        loop {
            let pending: i32 = self
                .evaluate("window.__eoka_pending_requests || 0")
                .await
                .unwrap_or(0);

            if pending == 0 {
                match idle_start {
                    Some(start) if start.elapsed() >= idle_duration => {
                        return Ok(());
                    }
                    None => {
                        idle_start = Some(std::time::Instant::now());
                    }
                    _ => {}
                }
            } else {
                idle_start = None;
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Network did not become idle within {}ms (pending: {})",
                    timeout_ms, pending
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    /// Get a list of all frames on the page
    pub async fn frames(&self) -> Result<Vec<FrameInfo>> {
        let frame_tree = self.session.get_frame_tree().await?;
        let mut frames = vec![FrameInfo {
            id: frame_tree.frame.id.clone(),
            url: frame_tree.frame.url.clone(),
            name: frame_tree.frame.name.clone(),
        }];

        fn collect_frames(children: &[crate::cdp::types::FrameTree], frames: &mut Vec<FrameInfo>) {
            for child in children {
                frames.push(FrameInfo {
                    id: child.frame.id.clone(),
                    url: child.frame.url.clone(),
                    name: child.frame.name.clone(),
                });
                collect_frames(&child.child_frames, frames);
            }
        }

        collect_frames(&frame_tree.child_frames, &mut frames);
        Ok(frames)
    }

    /// Execute JavaScript inside an iframe
    pub async fn evaluate_in_frame<T: serde::de::DeserializeOwned>(
        &self,
        frame_selector: &str,
        expression: &str,
    ) -> Result<T> {
        let escaped_frame = escape_js_string(frame_selector);
        let escaped_expr = escape_js_string(expression);

        // Use Function constructor instead of eval (less likely to be blocked by CSP)
        let js = format!(
            r#"
            (() => {{
                const iframe = document.querySelector('{escaped_frame}');
                if (!iframe || !iframe.contentWindow) throw new Error('Frame not found: {escaped_frame}');
                const fn = new iframe.contentWindow.Function('return (' + '{escaped_expr}' + ')');
                return fn.call(iframe.contentWindow);
            }})()
            "#,
            escaped_frame = escaped_frame,
            escaped_expr = escaped_expr
        );

        self.evaluate(&js).await
    }
    /// Retry an operation multiple times with delays between attempts
    pub async fn with_retry<F, Fut, T>(
        &self,
        attempts: u32,
        delay_ms: u64,
        operation: F,
    ) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error = String::new();

        for attempt in 1..=attempts {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = e.to_string();
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(Error::RetryExhausted {
            attempts,
            last_error,
        })
    }
    /// Take a debug screenshot and save it with a timestamp
    ///
    /// Saves to `StealthConfig::debug_dir` if set, otherwise current directory.
    /// Useful during development to understand page state.
    pub async fn debug_screenshot(&self, prefix: &str) -> Result<String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let filename = match &self.config.debug_dir {
            Some(dir) => {
                // Ensure directory exists
                std::fs::create_dir_all(dir)?;
                format!("{}/{}_{}.png", dir, prefix, timestamp)
            }
            None => format!("{}_{}.png", prefix, timestamp),
        };

        let screenshot = self.screenshot().await?;
        std::fs::write(&filename, screenshot)?;
        Ok(filename)
    }

    /// Log the current page state for debugging
    pub async fn debug_state(&self) -> Result<PageState> {
        let state: PageState = self
            .evaluate(
                r#"({
                url: location.href,
                title: document.title,
                input_count: document.querySelectorAll('input').length,
                button_count: document.querySelectorAll('button').length,
                link_count: document.querySelectorAll('a').length,
                form_count: document.querySelectorAll('form').length
            })"#,
            )
            .await
            .unwrap_or_else(|_| PageState {
                url: "unknown".to_string(),
                title: "unknown".to_string(),
                input_count: 0,
                button_count: 0,
                link_count: 0,
                form_count: 0,
            });
        Ok(state)
    }

    // ========== FILE UPLOAD ==========

    /// Upload a file to a file input element
    ///
    /// ```ignore
    /// page.upload_file("input[type='file']", "/path/to/file.pdf").await?;
    /// ```
    pub async fn upload_file(&self, selector: &str, path: &str) -> Result<()> {
        self.upload_files(selector, &[path]).await
    }

    /// Upload multiple files to a file input element
    ///
    /// ```ignore
    /// page.upload_files("input[type='file']", &["/path/to/a.pdf", "/path/to/b.pdf"]).await?;
    /// ```
    pub async fn upload_files(&self, selector: &str, paths: &[&str]) -> Result<()> {
        let element = self.find(selector).await?;
        let files: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        self.session
            .set_file_input_files(element.node_id, files)
            .await
    }

    // ========== SELECT / DROPDOWN ==========

    /// Select an option in a `<select>` element by value
    ///
    /// ```ignore
    /// page.select("#country", "US").await?;
    /// ```
    pub async fn select(&self, selector: &str, value: &str) -> Result<()> {
        let escaped_sel = escape_js_string(selector);
        let escaped_val = escape_js_string(value);
        self.execute(&format!(
            r#"(() => {{
                const el = document.querySelector('{escaped_sel}');
                if (!el) throw new Error('Select element not found: {escaped_sel}');
                const opt = Array.from(el.options).find(o => o.value === '{escaped_val}');
                if (!opt) throw new Error('Option value not found: {escaped_val}');
                el.value = '{escaped_val}';
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            }})()"#
        ))
        .await
    }

    /// Select an option in a `<select>` element by visible text
    ///
    /// ```ignore
    /// page.select_by_text("#country", "United States").await?;
    /// ```
    pub async fn select_by_text(&self, selector: &str, text: &str) -> Result<()> {
        let escaped_sel = escape_js_string(selector);
        let escaped_text = escape_js_string(text);
        self.execute(&format!(
            r#"(() => {{
                const el = document.querySelector('{escaped_sel}');
                if (!el) throw new Error('Select element not found');
                const opt = Array.from(el.options).find(o => o.text.trim() === '{escaped_text}');
                if (!opt) throw new Error('Option not found: {escaped_text}');
                el.value = opt.value;
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            }})()"#
        ))
        .await
    }

    /// Select multiple options in a `<select multiple>` element by values
    ///
    /// ```ignore
    /// page.select_multiple("#tags", &["rust", "async"]).await?;
    /// ```
    pub async fn select_multiple(&self, selector: &str, values: &[&str]) -> Result<()> {
        let escaped_sel = escape_js_string(selector);
        let values_json = serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
        self.execute(&format!(
            r#"(() => {{
                const el = document.querySelector('{escaped_sel}');
                if (!el) throw new Error('Select element not found');
                const values = {values_json};
                for (const opt of el.options) {{
                    opt.selected = values.includes(opt.value);
                }}
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            }})()"#
        ))
        .await
    }

    // ========== HOVER ==========

    /// Hover over an element (move mouse to it without clicking)
    ///
    /// Useful for revealing dropdown menus or tooltips.
    ///
    /// ```ignore
    /// page.hover("#menu-trigger").await?;
    /// page.click("#submenu-item").await?;
    /// ```
    pub async fn hover(&self, selector: &str) -> Result<()> {
        let element = self.find(selector).await?;
        let (x, y) = element.center().await?;
        self.session
            .dispatch_mouse_event(MouseEventType::MouseMoved, x, y, None, None)
            .await
    }

    /// Hover over an element with human-like mouse movement
    ///
    /// Uses Bezier curves to simulate natural mouse movement to the element.
    pub async fn human_hover(&self, selector: &str) -> Result<()> {
        let element = self.find(selector).await?;
        element.scroll_into_view().await?;
        let (target_x, target_y) = element.center().await?;

        // Use Human helper for realistic mouse movement (same as human_click but no click)
        let human = Human::new(&self.session);
        human.move_to(target_x, target_y).await?;

        // Brief pause to let hover effects trigger
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(())
    }

    // ========== KEYBOARD ==========

    /// Press a key (or key combination)
    ///
    /// Supports modifiers: `Ctrl+A`, `Cmd+C`, `Shift+Tab`, `Alt+F4`
    ///
    /// Common keys: `Enter`, `Tab`, `Escape`, `Backspace`, `Delete`,
    /// `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`,
    /// `Home`, `End`, `PageUp`, `PageDown`
    ///
    /// ```ignore
    /// page.press_key("Enter").await?;
    /// page.press_key("Ctrl+A").await?;
    /// page.press_key("Cmd+Shift+S").await?;
    /// ```
    pub async fn press_key(&self, key: &str) -> Result<()> {
        use crate::cdp::types::{InputDispatchKeyEventFull, KeyEventType};

        let (mods, key_name) = parse_key_combo(key);

        let (key_str, code_str, vk) = key_to_codes(key_name);

        // Key down
        self.session
            .dispatch_key_event_full(InputDispatchKeyEventFull {
                r#type: KeyEventType::KeyDown,
                modifiers: if mods != 0 { Some(mods) } else { None },
                key: Some(key_str.to_string()),
                code: Some(code_str.to_string()),
                windows_virtual_key_code: vk,
                native_virtual_key_code: vk,
                ..Default::default()
            })
            .await?;

        // Brief delay
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Key up
        self.session
            .dispatch_key_event_full(InputDispatchKeyEventFull {
                r#type: KeyEventType::KeyUp,
                modifiers: if mods != 0 { Some(mods) } else { None },
                key: Some(key_str.to_string()),
                code: Some(code_str.to_string()),
                windows_virtual_key_code: vk,
                native_virtual_key_code: vk,
                ..Default::default()
            })
            .await?;

        Ok(())
    }

    /// Press Enter key
    pub async fn press_enter(&self) -> Result<()> {
        self.press_key("Enter").await
    }

    /// Press Tab key
    pub async fn press_tab(&self) -> Result<()> {
        self.press_key("Tab").await
    }

    /// Press Escape key
    pub async fn press_escape(&self) -> Result<()> {
        self.press_key("Escape").await
    }

    /// Select all text (Ctrl+A / Cmd+A)
    pub async fn select_all(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return self.press_key("Cmd+A").await;
        #[cfg(not(target_os = "macos"))]
        return self.press_key("Ctrl+A").await;
    }

    /// Copy selection (Ctrl+C / Cmd+C)
    pub async fn copy(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return self.press_key("Cmd+C").await;
        #[cfg(not(target_os = "macos"))]
        return self.press_key("Ctrl+C").await;
    }

    /// Paste from clipboard (Ctrl+V / Cmd+V)
    pub async fn paste(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return self.press_key("Cmd+V").await;
        #[cfg(not(target_os = "macos"))]
        return self.press_key("Ctrl+V").await;
    }
}

/// Parse a key combination like "Ctrl+Shift+A" into (modifiers, key)
fn parse_key_combo(combo: &str) -> (i32, &str) {
    use crate::cdp::types::modifiers;

    let parts: Vec<&str> = combo.split('+').collect();
    let mut mods = 0;
    let mut key = combo;

    for (i, part) in parts.iter().enumerate() {
        let lower = part.to_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => mods |= modifiers::CTRL,
            "alt" | "option" => mods |= modifiers::ALT,
            "shift" => mods |= modifiers::SHIFT,
            "cmd" | "meta" | "command" => mods |= modifiers::META,
            _ => {
                // This is the actual key
                key = parts[i];
            }
        }
    }

    (mods, key)
}

/// Map key names to (key, code, virtualKeyCode)
fn key_to_codes(key: &str) -> (&str, &str, Option<i32>) {
    match key.to_lowercase().as_str() {
        "enter" | "return" => ("Enter", "Enter", Some(13)),
        "tab" => ("Tab", "Tab", Some(9)),
        "escape" | "esc" => ("Escape", "Escape", Some(27)),
        "backspace" => ("Backspace", "Backspace", Some(8)),
        "delete" => ("Delete", "Delete", Some(46)),
        "arrowup" | "up" => ("ArrowUp", "ArrowUp", Some(38)),
        "arrowdown" | "down" => ("ArrowDown", "ArrowDown", Some(40)),
        "arrowleft" | "left" => ("ArrowLeft", "ArrowLeft", Some(37)),
        "arrowright" | "right" => ("ArrowRight", "ArrowRight", Some(39)),
        "home" => ("Home", "Home", Some(36)),
        "end" => ("End", "End", Some(35)),
        "pageup" => ("PageUp", "PageUp", Some(33)),
        "pagedown" => ("PageDown", "PageDown", Some(34)),
        "space" | " " => (" ", "Space", Some(32)),
        "a" => ("a", "KeyA", Some(65)),
        "b" => ("b", "KeyB", Some(66)),
        "c" => ("c", "KeyC", Some(67)),
        "d" => ("d", "KeyD", Some(68)),
        "e" => ("e", "KeyE", Some(69)),
        "f" => ("f", "KeyF", Some(70)),
        "g" => ("g", "KeyG", Some(71)),
        "h" => ("h", "KeyH", Some(72)),
        "i" => ("i", "KeyI", Some(73)),
        "j" => ("j", "KeyJ", Some(74)),
        "k" => ("k", "KeyK", Some(75)),
        "l" => ("l", "KeyL", Some(76)),
        "m" => ("m", "KeyM", Some(77)),
        "n" => ("n", "KeyN", Some(78)),
        "o" => ("o", "KeyO", Some(79)),
        "p" => ("p", "KeyP", Some(80)),
        "q" => ("q", "KeyQ", Some(81)),
        "r" => ("r", "KeyR", Some(82)),
        "s" => ("s", "KeyS", Some(83)),
        "t" => ("t", "KeyT", Some(84)),
        "u" => ("u", "KeyU", Some(85)),
        "v" => ("v", "KeyV", Some(86)),
        "w" => ("w", "KeyW", Some(87)),
        "x" => ("x", "KeyX", Some(88)),
        "y" => ("y", "KeyY", Some(89)),
        "z" => ("z", "KeyZ", Some(90)),
        "f1" => ("F1", "F1", Some(112)),
        "f2" => ("F2", "F2", Some(113)),
        "f3" => ("F3", "F3", Some(114)),
        "f4" => ("F4", "F4", Some(115)),
        "f5" => ("F5", "F5", Some(116)),
        "f6" => ("F6", "F6", Some(117)),
        "f7" => ("F7", "F7", Some(118)),
        "f8" => ("F8", "F8", Some(119)),
        "f9" => ("F9", "F9", Some(120)),
        "f10" => ("F10", "F10", Some(121)),
        "f11" => ("F11", "F11", Some(122)),
        "f12" => ("F12", "F12", Some(123)),
        _ => (key, key, None),
    }
}

/// A captured HTTP request with its response
#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub request_id: String,
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub post_data: Option<String>,
    pub resource_type: Option<String>,
    pub status: Option<i32>,
    pub status_text: Option<String>,
    pub response_headers: Option<HashMap<String, String>>,
    pub mime_type: Option<String>,
    pub timestamp: f64,
    pub complete: bool,
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
}

/// Information about a frame/iframe
#[derive(Debug, Clone)]
pub struct FrameInfo {
    /// Frame ID
    pub id: String,
    /// Frame URL
    pub url: String,
    /// Frame name (if any)
    pub name: Option<String>,
}

/// Debug information about page state
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PageState {
    pub url: String,
    pub title: String,
    pub input_count: u32,
    pub button_count: u32,
    pub link_count: u32,
    pub form_count: u32,
}

/// Bounding box of an element
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl BoundingBox {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// An element on the page (holds a CDP node_id, can become stale on DOM changes)
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

    /// Get inner text
    ///
    /// Extracts text content from the element's outerHTML without using focus.
    pub async fn text(&self) -> Result<String> {
        let value = self.eval_on_element("this.textContent || ''").await?;

        if let Some(s) = value.as_str() {
            return Ok(s.to_string());
        }
        Ok(String::new())
    }

    /// Evaluate a JavaScript expression on this element via Runtime.callFunctionOn.
    ///
    /// The expression should use `this` to refer to the element.
    /// Example: `"this.textContent || ''"`, `"this.tagName.toLowerCase()"`
    async fn eval_on_element(&self, js_body: &str) -> Result<serde_json::Value> {
        let object_id = self.page.session.resolve_node(self.node_id).await?;

        let func = format!("function() {{ return {}; }}", js_body);

        let result = self
            .page
            .session
            .call_function_on(&object_id, &func)
            .await?;
        Ok(result.result.value.unwrap_or(serde_json::Value::Null))
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
    /// Check if the element is visible (has a computable box model)
    pub async fn is_visible(&self) -> Result<bool> {
        match self.page.session.get_box_model(self.node_id).await {
            Ok(_) => Ok(true),
            Err(Error::Cdp { message, .. }) if message.contains("box model") => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Get the element's bounding box
    ///
    /// Returns None if the element is not visible/rendered.
    pub async fn bounding_box(&self) -> Option<BoundingBox> {
        match self.page.session.get_box_model(self.node_id).await {
            Ok(model) => {
                let content = &model.content;
                if content.len() >= 8 {
                    // content is [x1,y1, x2,y2, x3,y3, x4,y4] for a quad
                    // Handle rotated/transformed elements by finding actual bounds
                    let xs = [content[0], content[2], content[4], content[6]];
                    let ys = [content[1], content[3], content[5], content[7]];

                    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
                    let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
                    let max_y = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);

                    Some(BoundingBox {
                        x: min_x,
                        y: min_y,
                        width: max_x - min_x,
                        height: max_y - min_y,
                    })
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }

    /// Get an attribute value
    pub async fn get_attribute(&self, name: &str) -> Result<Option<String>> {
        let escaped_name = escape_js_string(name);
        let value = self
            .eval_on_element(&format!("this.getAttribute('{}')", escaped_name))
            .await?;

        if value.is_null() {
            return Ok(None);
        }
        if let Some(s) = value.as_str() {
            return Ok(Some(s.to_string()));
        }
        Ok(None)
    }

    /// Get the tag name of the element (e.g., "div", "input", "a")
    pub async fn tag_name(&self) -> Result<String> {
        let value = self.eval_on_element("this.tagName.toLowerCase()").await?;

        if let Some(s) = value.as_str() {
            return Ok(s.to_string());
        }
        Ok(String::new())
    }

    /// Check if the element is enabled (not disabled)
    pub async fn is_enabled(&self) -> Result<bool> {
        let value = self.eval_on_element("!this.disabled").await?;

        if let Some(b) = value.as_bool() {
            return Ok(b);
        }
        Ok(true) // Default to enabled if we can't determine
    }

    /// Check if a checkbox/radio is checked
    pub async fn is_checked(&self) -> Result<bool> {
        let value = self.eval_on_element("this.checked === true").await?;

        if let Some(b) = value.as_bool() {
            return Ok(b);
        }
        Ok(false)
    }

    /// Get the value of an input element
    pub async fn value(&self) -> Result<String> {
        let value = self.eval_on_element("this.value || ''").await?;

        if let Some(s) = value.as_str() {
            return Ok(s.to_string());
        }
        Ok(String::new())
    }

    /// Get computed CSS property value
    pub async fn css(&self, property: &str) -> Result<String> {
        let escaped = escape_js_string(property);
        let value = self
            .eval_on_element(&format!(
                "getComputedStyle(this).getPropertyValue('{}')",
                escaped
            ))
            .await?;

        if let Some(s) = value.as_str() {
            return Ok(s.to_string());
        }
        Ok(String::new())
    }

    /// Scroll this element into view
    pub async fn scroll_into_view(&self) -> Result<()> {
        let object_id = self.page.session.resolve_node(self.node_id).await?;
        self.page
            .session
            .call_function_on(
                &object_id,
                "function() { this.scrollIntoView({ behavior: 'smooth', block: 'center' }); }",
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_combo_simple() {
        let (mods, key) = parse_key_combo("Enter");
        assert_eq!(mods, 0);
        assert_eq!(key, "Enter");
    }

    #[test]
    fn test_parse_key_combo_ctrl() {
        use crate::cdp::types::modifiers;
        let (mods, key) = parse_key_combo("Ctrl+A");
        assert_eq!(mods, modifiers::CTRL);
        assert_eq!(key, "A");
    }

    #[test]
    fn test_parse_key_combo_cmd_shift() {
        use crate::cdp::types::modifiers;
        let (mods, key) = parse_key_combo("Cmd+Shift+S");
        assert_eq!(mods, modifiers::META | modifiers::SHIFT);
        assert_eq!(key, "S");
    }

    #[test]
    fn test_parse_key_combo_all_modifiers() {
        use crate::cdp::types::modifiers;
        let (mods, key) = parse_key_combo("Ctrl+Alt+Shift+Cmd+X");
        assert_eq!(
            mods,
            modifiers::CTRL | modifiers::ALT | modifiers::SHIFT | modifiers::META
        );
        assert_eq!(key, "X");
    }

    #[test]
    fn test_parse_key_combo_case_insensitive() {
        use crate::cdp::types::modifiers;
        let (mods, key) = parse_key_combo("ctrl+a");
        assert_eq!(mods, modifiers::CTRL);
        assert_eq!(key, "a");
    }

    #[test]
    fn test_key_to_codes_enter() {
        let (key, code, vk) = key_to_codes("Enter");
        assert_eq!(key, "Enter");
        assert_eq!(code, "Enter");
        assert_eq!(vk, Some(13));
    }

    #[test]
    fn test_key_to_codes_tab() {
        let (key, code, vk) = key_to_codes("Tab");
        assert_eq!(key, "Tab");
        assert_eq!(code, "Tab");
        assert_eq!(vk, Some(9));
    }

    #[test]
    fn test_key_to_codes_letter() {
        let (key, code, vk) = key_to_codes("a");
        assert_eq!(key, "a");
        assert_eq!(code, "KeyA");
        assert_eq!(vk, Some(65));
    }

    #[test]
    fn test_key_to_codes_arrow() {
        let (key, code, vk) = key_to_codes("ArrowDown");
        assert_eq!(key, "ArrowDown");
        assert_eq!(code, "ArrowDown");
        assert_eq!(vk, Some(40));
    }

    #[test]
    fn test_key_to_codes_case_insensitive() {
        let (key, code, vk) = key_to_codes("ESCAPE");
        assert_eq!(key, "Escape");
        assert_eq!(code, "Escape");
        assert_eq!(vk, Some(27));
    }

    #[test]
    fn test_key_to_codes_alias() {
        // "esc" should work as alias for "Escape"
        let (key, code, vk) = key_to_codes("esc");
        assert_eq!(key, "Escape");
        assert_eq!(code, "Escape");
        assert_eq!(vk, Some(27));

        // "up" should work as alias for "ArrowUp"
        let (key, code, vk) = key_to_codes("up");
        assert_eq!(key, "ArrowUp");
        assert_eq!(code, "ArrowUp");
        assert_eq!(vk, Some(38));
    }

    #[test]
    fn test_key_to_codes_unknown() {
        // Unknown keys should pass through
        let (key, code, vk) = key_to_codes("SomeWeirdKey");
        assert_eq!(key, "SomeWeirdKey");
        assert_eq!(code, "SomeWeirdKey");
        assert_eq!(vk, None);
    }

    #[test]
    fn test_escape_js_string() {
        assert_eq!(escape_js_string("hello"), "hello");
        assert_eq!(escape_js_string("it's"), "it\\'s");
        assert_eq!(escape_js_string("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_js_string("back\\slash"), "back\\\\slash");
        assert_eq!(escape_js_string("${var}"), "\\${var}");
    }
}
