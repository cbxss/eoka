//! Page Abstraction
//!
//! High-level API for interacting with a browser page.

use std::collections::HashMap;
use std::sync::Arc;

use crate::cdp::{Cookie, MouseButton, MouseEventType, Session};
use crate::error::{Error, Result};
use crate::keyboard::{key_to_codes, parse_key_combo};
use crate::stealth::Human;
use crate::StealthConfig;

// Re-export so the historical `eoka::page::Element` / `eoka::page::BoundingBox`
// paths keep resolving after the split into `element.rs`.
pub use crate::element::{BoundingBox, Element};

/// Polling interval (ms) used by wait_for_* loop iterations.
const POLL_INTERVAL_MS: u64 = 100;

/// Settle time (ms) used after actions (click, navigate, hover) to let the
/// page react before the next step.
const SETTLE_MS: u64 = 100;

/// Brief pause (ms) between micro-interactions (e.g. focus→type, select_all→delete).
pub(crate) const INTERACTION_DELAY_MS: u64 = 50;

/// Async sleep for the given number of milliseconds.
pub(crate) async fn sleep_ms(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Escape a string for safe use in JavaScript string literals (single pass).
pub(crate) fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '`' => out.push_str("\\`"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\x00"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            '$' if chars.peek() == Some(&'{') => {
                out.push_str("\\${");
                chars.next();
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Whether a CDP error message names a missing/stale backend node.
fn is_missing_node_message(message: &str) -> bool {
    message.contains("Could not find node") || message.contains("No node with given id")
}

/// Check if a CDP error indicates the cached node id is no longer valid.
fn is_stale_node_error(e: &Error) -> bool {
    matches!(e, Error::Cdp { message, .. } if is_missing_node_message(message))
}

/// Check if a CDP error is an element-related error (not found, not visible, etc.)
fn is_element_cdp_error(e: &Error) -> bool {
    match e {
        Error::ElementNotFound(_) | Error::ElementNotVisible { .. } => true,
        Error::Cdp { message, .. } => {
            message.contains("box model")
                || is_missing_node_message(message)
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

/// A browser page with stealth capabilities. Cheap to clone (`Arc`-backed
/// shared state) — clones refer to the same tab and share the document cache.
#[derive(Clone)]
pub struct Page {
    pub(crate) session: Session,
    config: Arc<StealthConfig>,
    /// Cached root `DOM.getDocument` node id (invalidated on navigation).
    /// `Arc` so `Element` clones share one page's cache.
    root_node: Arc<std::sync::Mutex<Option<i32>>>,
    /// Randomized per-page property key for the network-idle request counter.
    net_idle_key: String,
}

impl Page {
    /// Create a new Page wrapping a CDP session
    pub(crate) fn new(session: Session, config: Arc<StealthConfig>) -> Self {
        let net_idle_key = format!("_{:012x}", fastrand::u64(..) & 0xffff_ffff_ffff);
        Self {
            session,
            config,
            root_node: Arc::new(std::sync::Mutex::new(None)),
            net_idle_key,
        }
    }

    /// Return the cached root document node id, fetching it once if needed.
    async fn document_node(&self) -> Result<i32> {
        if let Some(id) = *self.root_node.lock().unwrap() {
            return Ok(id);
        }
        let doc = self.session.get_document(Some(0)).await?;
        *self.root_node.lock().unwrap() = Some(doc.node_id);
        Ok(doc.node_id)
    }

    /// Drop the cached document node (call after navigation).
    fn invalidate_document(&self) {
        *self.root_node.lock().unwrap() = None;
    }

    /// Run an operation against the cached document node, retrying once if stale.
    async fn with_document<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(i32) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let doc = self.document_node().await?;
        match op(doc).await {
            Err(ref e) if is_stale_node_error(e) => {
                self.invalidate_document();
                let doc = self.document_node().await?;
                op(doc).await
            }
            other => other,
        }
    }

    /// Get the underlying CDP session
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Get the target ID for this page (tab identifier)
    pub fn target_id(&self) -> &str {
        self.session.target_id()
    }

    /// Check a navigation result for errors.
    /// ERR_HTTP_RESPONSE_CODE_FAILURE (4xx/5xx) is ignored — the page still loaded.
    fn check_nav_result(result: &crate::cdp::types::PageNavigateResult) -> Result<()> {
        if let Some(ref error) = result.error_text {
            if error != "net::ERR_HTTP_RESPONSE_CODE_FAILURE" {
                return Err(Error::Navigation(error.clone()));
            }
        }
        Ok(())
    }

    /// Resolve the current page URL for cookie operations.
    /// Returns None for about:blank (Chrome silently rejects cookies without a URL).
    async fn cookie_url(&self) -> Result<Option<String>> {
        let url = self.url().await?;
        Ok(if url == "about:blank" {
            None
        } else {
            Some(url)
        })
    }

    /// Navigate to a URL
    pub async fn goto(&self, url: &str) -> Result<()> {
        self.navigate_impl(url, None).await
    }

    /// Reload the page
    pub async fn reload(&self) -> Result<()> {
        self.session.reload(false).await?;
        self.invalidate_document();
        Ok(())
    }

    /// Go back in history
    pub async fn back(&self) -> Result<()> {
        self.session.go_back().await?;
        self.invalidate_document();
        Ok(())
    }

    /// Go forward in history
    pub async fn forward(&self) -> Result<()> {
        self.session.go_forward().await?;
        self.invalidate_document();
        Ok(())
    }
    /// Get current URL
    pub async fn url(&self) -> Result<String> {
        let frame_tree = self.session.get_frame_tree().await?;
        Ok(frame_tree.frame.url)
    }

    /// Get page title
    pub async fn title(&self) -> Result<String> {
        // Use evaluate_sync (awaitPromise=false) so heavy SPAs don't block
        self.evaluate_sync("document.title || ''").await
    }

    /// Get page HTML content
    pub async fn content(&self) -> Result<String> {
        self.evaluate_sync("document.documentElement.outerHTML")
            .await
    }

    /// Get page text content (body innerText)
    pub async fn text(&self) -> Result<String> {
        self.evaluate_sync("document.body?.innerText || ''").await
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
    pub async fn find(&self, selector: &str) -> Result<Element> {
        let node_id = self
            .with_document(|doc| self.session.query_selector(doc, selector))
            .await?;

        if node_id == 0 {
            return Err(Error::ElementNotFound(selector.to_string()));
        }

        Ok(Element {
            page: self.clone(),
            node_id,
        })
    }

    /// Find all elements matching a CSS selector
    pub async fn find_all(&self, selector: &str) -> Result<Vec<Element>> {
        let node_ids = self
            .with_document(|doc| self.session.query_selector_all(doc, selector))
            .await?;

        Ok(node_ids
            .into_iter()
            .filter(|&id| id != 0)
            .map(|node_id| Element {
                page: self.clone(),
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
    pub async fn find_by_text(&self, text: &str) -> Result<Element> {
        self.find_by_text_match(text, TextMatch::Contains).await
    }

    /// Find an element by text with specific matching strategy
    ///
    /// Prioritizes interactive elements (a, button, input) over static elements.
    /// Uses Runtime.callFunctionOn to avoid mutating the DOM (no marker attributes).
    pub async fn find_by_text_match(&self, text: &str, match_type: TextMatch) -> Result<Element> {
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
        );

        let result = self.session.evaluate_for_remote_object(&js).await?;
        let remote = self.check_js_result(result)?;

        if remote.subtype.as_deref() == Some("null") {
            return Err(Error::ElementNotFound(format!("text: {}", text)));
        }

        let object_id = remote
            .object_id
            .ok_or_else(|| Error::ElementNotFound(format!("text: {}", text)))?;

        // Prime the DOM node-id space (DOM.requestNode returns 0 unless
        // DOM.getDocument has populated it for the current document).
        self.document_node().await?;

        // Convert remote object to DOM node_id
        let node_id = self.session.request_node(&object_id).await?;

        if node_id == 0 {
            return Err(Error::ElementNotFound(format!("text: {}", text)));
        }

        Ok(Element {
            page: self.clone(),
            node_id,
        })
    }

    /// Find all elements matching the given text
    pub async fn find_all_by_text(&self, text: &str) -> Result<Vec<Element>> {
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

        // Prime the DOM node-id space (DOM.requestNode returns 0 otherwise).
        self.document_node().await?;

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
                            page: self.clone(),
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

        sleep_ms(INTERACTION_DELAY_MS).await;

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
        sleep_ms(INTERACTION_DELAY_MS).await;
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
        self.try_click_impl(self.find(selector).await).await
    }

    /// Try to click an element by text, returning Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_click_by_text(&self, text: &str) -> Result<bool> {
        self.try_click_impl(self.find_by_text(text).await).await
    }

    /// Shared impl for try_click and try_click_by_text
    async fn try_click_impl(&self, find_result: Result<Element>) -> Result<bool> {
        match find_result {
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
        sleep_ms(INTERACTION_DELAY_MS).await;

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
        self.human_click(selector).await?;
        sleep_ms(SETTLE_MS).await;
        self.human_type_text(text).await
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
        self.human_click(selector).await?;
        sleep_ms(SETTLE_MS).await;
        let escaped = escape_js_string(selector);
        let select_js = format!(
            "(() => {{ const el = document.querySelector('{escaped}'); \
             if (el) {{ el.focus(); if (typeof el.select === 'function') el.select(); }} }})()"
        );
        self.execute(&select_js).await?;
        sleep_ms(INTERACTION_DELAY_MS).await;
        self.human_type_text(value).await
    }

    /// Type text, using human-like timing if configured
    async fn human_type_text(&self, text: &str) -> Result<()> {
        if self.config.human_typing {
            self.human().type_text(text).await
        } else {
            self.session.insert_text(text).await
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
    async fn try_human_click_impl(&self, find_result: Result<Element>) -> Result<bool> {
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
        self.eval_impl(self.session.evaluate(expression).await?)
    }

    /// Evaluate JavaScript synchronously (don't await promises).
    /// Use when the page may have unresolved promises that block normal evaluate.
    pub async fn evaluate_sync<T: serde::de::DeserializeOwned>(
        &self,
        expression: &str,
    ) -> Result<T> {
        self.eval_impl(self.session.evaluate_sync(expression).await?)
    }

    /// Shared impl: check for exceptions and extract the value
    fn eval_impl<T: serde::de::DeserializeOwned>(
        &self,
        result: crate::cdp::types::RuntimeEvaluateResult,
    ) -> Result<T> {
        let remote = self.check_js_result(result)?;
        let value = remote
            .value
            .ok_or_else(|| Error::cdp_msg("No value returned from evaluate"))?;
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
            return Err(Error::cdp_msg(format!(
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
        let url = self.cookie_url().await?;
        let success = self
            .session
            .set_cookie(name, value, url.as_deref(), domain, path)
            .await?;
        if !success {
            return Err(Error::cdp_msg("Failed to set cookie"));
        }
        Ok(())
    }

    /// Delete a cookie
    pub async fn delete_cookie(&self, name: &str, domain: Option<&str>) -> Result<()> {
        let url = self.cookie_url().await?;
        self.session
            .delete_cookies(name, url.as_deref(), domain)
            .await
    }

    /// Clear all browser cookies for this context.
    pub async fn clear_all_cookies(&self) -> Result<()> {
        self.session.clear_all_cookies().await
    }

    /// Bulk-import cookies (e.g., restored from a prior dump_storage call).
    pub async fn set_cookies_bulk(
        &self,
        cookies: Vec<crate::cdp::types::NetworkSetCookie>,
    ) -> Result<()> {
        self.session.set_cookies(cookies).await
    }

    /// Set extra HTTP headers sent with every subsequent request from this page.
    /// Pass an empty map to clear.
    pub async fn set_extra_headers(&self, headers: HashMap<String, String>) -> Result<()> {
        self.session.set_extra_headers(headers).await
    }

    /// Remove all extra HTTP headers set via set_extra_headers.
    pub async fn clear_extra_headers(&self) -> Result<()> {
        self.session.clear_extra_headers().await
    }

    /// Navigate to `url` while sending custom HTTP headers.
    /// Headers are set before navigation and cleared afterward.
    pub async fn goto_with_headers(
        &self,
        url: &str,
        headers: HashMap<String, String>,
    ) -> Result<()> {
        self.session.set_extra_headers(headers).await?;
        let result = self.goto(url).await;
        let _ = self.session.clear_extra_headers().await;
        result
    }

    /// Navigate to `url` with a custom Referer header.
    pub async fn goto_with_referrer(&self, url: &str, referrer: &str) -> Result<()> {
        self.navigate_impl(url, Some(referrer)).await
    }

    /// Shared navigation impl
    async fn navigate_impl(&self, url: &str, referrer: Option<&str>) -> Result<()> {
        let result = self.session.navigate(url, referrer).await?;
        Self::check_nav_result(&result)?;
        self.invalidate_document();
        self.wait_for_load(self.config.cdp_timeout.saturating_mul(1000))
            .await;
        Ok(())
    }

    /// Poll `document.readyState` until interactive/complete or the budget elapses.
    async fn wait_for_load(&self, timeout_ms: u64) {
        let start = std::time::Instant::now();
        loop {
            let state: String = self
                .evaluate_sync("document.readyState")
                .await
                .unwrap_or_default();
            if state == "complete" || state == "interactive" {
                break;
            }
            if start.elapsed().as_millis() as u64 >= timeout_ms {
                break;
            }
            sleep_ms(POLL_INTERVAL_MS).await;
        }
        sleep_ms(SETTLE_MS).await;
    }

    /// Disable CSP enforcement for the current page.
    /// Must be called before navigation to take effect.
    pub async fn set_bypass_csp(&self, enabled: bool) -> Result<()> {
        self.session.set_bypass_csp(enabled).await
    }

    /// Override the User-Agent string for this page.
    pub async fn set_user_agent(&self, user_agent: &str) -> Result<()> {
        self.session.set_user_agent(user_agent, None).await
    }

    /// Ignore TLS certificate errors for this session.
    pub async fn ignore_cert_errors(&self, ignore: bool) -> Result<()> {
        self.session.set_ignore_cert_errors(ignore).await
    }

    /// Accept a pending JS dialog (alert / confirm / prompt).
    /// For prompt() dialogs, provide `prompt_text` to fill the input.
    pub async fn accept_dialog(&self, prompt_text: Option<&str>) -> Result<()> {
        self.session.handle_dialog(true, prompt_text).await
    }

    /// Dismiss a pending JS dialog (cancel / close).
    pub async fn dismiss_dialog(&self) -> Result<()> {
        self.session.handle_dialog(false, None).await
    }

    /// Generic polling helper — calls `check` every `POLL_INTERVAL_MS` until it
    /// returns `Ok(Some(value))`, then returns that value.  Returns `Err(Timeout)`
    /// if `timeout_ms` elapses first.
    async fn poll_until<T, F, Fut>(&self, timeout_ms: u64, error_msg: String, check: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        loop {
            if let Some(val) = check().await {
                return Ok(val);
            }
            if start.elapsed() > timeout {
                return Err(Error::Timeout(error_msg));
            }
            sleep_ms(POLL_INTERVAL_MS).await;
        }
    }

    /// Wait for an element to appear in the DOM
    pub async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<Element> {
        self.poll_until(
            timeout_ms,
            format!("Element '{}' not found within {}ms", selector, timeout_ms),
            || async { self.find(selector).await.ok() },
        )
        .await
    }

    /// Wait for an element to be visible and clickable
    pub async fn wait_for_visible(&self, selector: &str, timeout_ms: u64) -> Result<Element> {
        self.poll_until(
            timeout_ms,
            format!("Element '{}' not visible within {}ms", selector, timeout_ms),
            || async {
                if let Ok(elem) = self.find(selector).await {
                    if elem.center().await.is_ok() {
                        return Some(elem);
                    }
                }
                None
            },
        )
        .await
    }

    /// Wait for an element to disappear
    pub async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<()> {
        self.poll_until(
            timeout_ms,
            format!(
                "Element '{}' still visible after {}ms",
                selector, timeout_ms
            ),
            || async {
                match self.find(selector).await {
                    Err(Error::ElementNotFound(_)) => Some(()),
                    Ok(el) => match el.is_visible().await {
                        Ok(false) => Some(()),
                        _ => None,
                    },
                    Err(_) => None,
                }
            },
        )
        .await
    }

    /// Wait for a fixed duration
    pub async fn wait(&self, ms: u64) {
        sleep_ms(ms).await;
    }

    /// Wait for an element with specific text to appear
    pub async fn wait_for_text(&self, text: &str, timeout_ms: u64) -> Result<Element> {
        self.poll_until(
            timeout_ms,
            format!(
                "Element with text '{}' not found within {}ms",
                text, timeout_ms
            ),
            || async { self.find_by_text(text).await.ok() },
        )
        .await
    }

    /// Wait for the URL to contain a specific string
    pub async fn wait_for_url_contains(&self, pattern: &str, timeout_ms: u64) -> Result<()> {
        self.poll_until(
            timeout_ms,
            format!("URL did not contain '{}' within {}ms", pattern, timeout_ms),
            || async {
                if let Ok(url) = self.url().await {
                    if url.contains(pattern) {
                        return Some(());
                    }
                }
                None
            },
        )
        .await
    }

    /// Wait for URL to change from current URL
    pub async fn wait_for_url_change(&self, timeout_ms: u64) -> Result<String> {
        let original_url = self.url().await?;
        self.poll_until(
            timeout_ms,
            format!(
                "URL did not change from '{}' within {}ms",
                original_url, timeout_ms
            ),
            || async {
                if let Ok(url) = self.url().await {
                    if url != original_url {
                        return Some(url);
                    }
                }
                None
            },
        )
        .await
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
    pub async fn find_any(&self, selectors: &[&str]) -> Result<Element> {
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
    pub async fn wait_for_any(&self, selectors: &[&str], timeout_ms: u64) -> Result<Element> {
        self.poll_until(
            timeout_ms,
            format!(
                "None of selectors found within {}ms: {:?}",
                timeout_ms, selectors
            ),
            || async { self.find_any(selectors).await.ok() },
        )
        .await
    }
    /// Wait for network to become idle (no pending XHR/fetch for `idle_time_ms`)
    pub async fn wait_for_network_idle(&self, idle_time_ms: u64, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        let idle_duration = std::time::Duration::from_millis(idle_time_ms);

        let key = &self.net_idle_key;
        let check_idle_js = format!(
            r#"
            (() => {{
                var K = '{key}';
                if (window[K] === undefined) {{
                    Object.defineProperty(window, K, {{
                        value: {{ n: 0 }}, writable: true, enumerable: false, configurable: true
                    }});
                    var st = window[K];
                    const of = window.fetch;
                    const wf = function fetch(...a) {{
                        st.n++;
                        return of.apply(this, a).finally(() => {{ st.n--; }});
                    }};
                    try {{ wf.toString = () => 'function fetch() {{ [native code] }}'; }} catch(e) {{}}
                    window.fetch = wf;
                    const oo = XMLHttpRequest.prototype.open;
                    const os = XMLHttpRequest.prototype.send;
                    XMLHttpRequest.prototype.open = function(...a) {{ this[K] = true; return oo.apply(this, a); }};
                    XMLHttpRequest.prototype.send = function(...a) {{
                        if (this[K]) {{
                            st.n++;
                            this.addEventListener('loadend', () => {{ st.n--; }});
                        }}
                        return os.apply(this, a);
                    }};
                }}
                var pend = window[K].n;
                return (document.readyState === 'complete') ? pend : -1;
            }})()
        "#
        );

        let mut idle_start: Option<std::time::Instant> = None;

        loop {
            let pending: i32 = self.evaluate_sync(&check_idle_js).await.unwrap_or(-1);

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
                tracing::warn!(
                    "wait_for_network_idle timed out after {}ms with {} pending request(s)",
                    timeout_ms,
                    pending
                );
                return Err(Error::Timeout(format!(
                    "Network not idle after {}ms ({} pending requests)",
                    timeout_ms, pending
                )));
            }

            sleep_ms(INTERACTION_DELAY_MS).await;
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

    /// Execute JavaScript inside an iframe.
    ///
    /// # Safety
    ///
    /// `expression` is evaluated as **code** (via the `Function` constructor),
    /// not as a string literal. Do not pass untrusted user input as the
    /// expression — it will be executed in the iframe's JS context.
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
                const _exec = new iframe.contentWindow.Function('return (' + '{escaped_expr}' + ')');
                return _exec.call(iframe.contentWindow);
            }})()
            "#,
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
        let attempts = attempts.max(1);
        let mut last_error = String::new();

        for attempt in 1..=attempts {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = e.to_string();
                    if attempt < attempts {
                        sleep_ms(delay_ms).await;
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

    /// Upload file(s) to a file input element
    pub async fn upload_file(&self, selector: &str, path: &str) -> Result<()> {
        self.upload_files(selector, &[path]).await
    }

    /// Upload multiple files to a file input element
    pub async fn upload_files(&self, selector: &str, paths: &[&str]) -> Result<()> {
        let element = self.find(selector).await?;
        self.session
            .set_file_input_files(
                element.node_id,
                paths.iter().map(|p| p.to_string()).collect(),
            )
            .await
    }

    /// Select option by value
    pub async fn select(&self, selector: &str, value: &str) -> Result<()> {
        let (sel, val) = (escape_js_string(selector), escape_js_string(value));
        self.execute(&format!(
            r#"(()=>{{const el=document.querySelector('{sel}');if(!el)throw new Error('Select not found');const opt=[...el.options].find(o=>o.value==='{val}');if(!opt)throw new Error('Option not found: {val}');el.value='{val}';el.dispatchEvent(new Event('change',{{bubbles:true}}))}})()"#
        )).await
    }

    /// Select option by visible text
    pub async fn select_by_text(&self, selector: &str, text: &str) -> Result<()> {
        let (sel, txt) = (escape_js_string(selector), escape_js_string(text));
        self.execute(&format!(
            r#"(()=>{{const el=document.querySelector('{sel}');if(!el)throw new Error('Select not found');const opt=[...el.options].find(o=>o.text.trim()==='{txt}');if(!opt)throw new Error('Option not found: {txt}');el.value=opt.value;el.dispatchEvent(new Event('change',{{bubbles:true}}))}})()"#
        )).await
    }

    /// Select multiple options by values
    pub async fn select_multiple(&self, selector: &str, values: &[&str]) -> Result<()> {
        let sel = escape_js_string(selector);
        let vals = serde_json::to_string(values).unwrap_or_else(|_| "[]".into());
        self.execute(&format!(
            r#"(()=>{{const el=document.querySelector('{sel}');if(!el)throw new Error('Select not found');const v={vals};for(const o of el.options)o.selected=v.includes(o.value);el.dispatchEvent(new Event('change',{{bubbles:true}}))}})()"#
        )).await
    }

    /// Hover over element (for revealing menus)
    pub async fn hover(&self, selector: &str) -> Result<()> {
        let (x, y) = self.find(selector).await?.center().await?;
        self.session
            .dispatch_mouse_event(MouseEventType::MouseMoved, x, y, None, None)
            .await
    }

    /// Human-like hover with Bezier curve movement
    pub async fn human_hover(&self, selector: &str) -> Result<()> {
        let element = self.find(selector).await?;
        element.scroll_into_view().await?;
        let (x, y) = element.center().await?;
        Human::new(&self.session).move_to(x, y).await?;
        sleep_ms(SETTLE_MS).await;
        Ok(())
    }

    /// Press key with optional modifiers (e.g., "Enter", "Ctrl+A", "Cmd+Shift+S")
    pub async fn press_key(&self, key: &str) -> Result<()> {
        use crate::cdp::types::{InputDispatchKeyEventFull, KeyEventType};

        let (mods, key_name) = parse_key_combo(key);
        let (key_str, code_str, vk) = key_to_codes(key_name);
        let modifiers = if mods != 0 { Some(mods) } else { None };

        let make_event = |event_type| InputDispatchKeyEventFull {
            r#type: event_type,
            modifiers,
            key: Some(key_str.into()),
            code: Some(code_str.into()),
            windows_virtual_key_code: vk,
            native_virtual_key_code: vk,
            ..Default::default()
        };

        self.session
            .dispatch_key_event_full(make_event(KeyEventType::KeyDown))
            .await?;
        sleep_ms(INTERACTION_DELAY_MS).await;
        self.session
            .dispatch_key_event_full(make_event(KeyEventType::KeyUp))
            .await
    }

    /// Platform-aware select all (Cmd+A on Mac, Ctrl+A elsewhere)
    pub async fn select_all(&self) -> Result<()> {
        self.press_key(if cfg!(target_os = "macos") {
            "Cmd+A"
        } else {
            "Ctrl+A"
        })
        .await
    }

    /// Platform-aware copy (Cmd+C on Mac, Ctrl+C elsewhere)
    pub async fn copy(&self) -> Result<()> {
        self.press_key(if cfg!(target_os = "macos") {
            "Cmd+C"
        } else {
            "Ctrl+C"
        })
        .await
    }

    /// Platform-aware paste (Cmd+V on Mac, Ctrl+V elsewhere)
    pub async fn paste(&self) -> Result<()> {
        self.press_key(if cfg!(target_os = "macos") {
            "Cmd+V"
        } else {
            "Ctrl+V"
        })
        .await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_js_string() {
        assert_eq!(escape_js_string("hello"), "hello");
        assert_eq!(escape_js_string("it's"), "it\\'s");
        assert_eq!(escape_js_string("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_js_string("back\\slash"), "back\\\\slash");
        assert_eq!(escape_js_string("${var}"), "\\${var}");
        // Null byte escaped as \x00
        assert_eq!(escape_js_string("a\0b"), "a\\x00b");
        assert_eq!(escape_js_string("a\0"), "a\\x00");
        assert_eq!(escape_js_string("a\u{0}1"), "a\\x001");
        assert_eq!(escape_js_string("a\u{2028}b"), "a\\u2028b");
        assert_eq!(escape_js_string("a\u{2029}b"), "a\\u2029b");
    }

    #[test]
    fn test_escape_js_string_null_is_hex_not_octal() {
        // '\0' must become the hex escape \x00, never the octal \0 (which
        // would be ambiguous when followed by a digit and could be parsed
        // as an octal escape by a JS engine).
        assert_eq!(escape_js_string("\0"), "\\x00");
        assert_ne!(escape_js_string("\0"), "\\0");
        // Followed by a digit: must stay "\x001", not "\01".
        assert_eq!(escape_js_string("a\u{0}1"), "a\\x001");
    }

    #[test]
    fn test_escape_js_string_all_special_chars() {
        assert_eq!(escape_js_string("'"), "\\'");
        assert_eq!(escape_js_string("\""), "\\\"");
        assert_eq!(escape_js_string("`"), "\\`");
        assert_eq!(escape_js_string("\\"), "\\\\");
        assert_eq!(escape_js_string("\n"), "\\n");
        assert_eq!(escape_js_string("\r"), "\\r");
        assert_eq!(escape_js_string("${"), "\\${");
        assert_eq!(escape_js_string("\u{2028}"), "\\u2028");
        assert_eq!(escape_js_string("\u{2029}"), "\\u2029");
    }

    #[test]
    fn test_stale_node_error_detection() {
        let stale = Error::cdp(
            "DOM.resolveNode",
            -32000,
            "Could not find node with given id",
        );
        assert!(is_stale_node_error(&stale));
        assert!(is_element_cdp_error(&stale));
    }

    #[test]
    fn test_box_model_error_is_element_but_not_stale() {
        let box_model = Error::cdp("DOM.getBoxModel", -32000, "box model could not be computed");
        // "box model" substring makes it an element error...
        assert!(is_element_cdp_error(&box_model));
        // ...but it is not a stale-node error.
        assert!(!is_stale_node_error(&box_model));
    }

    #[test]
    fn test_element_not_found_is_element_but_not_stale() {
        let not_found = Error::ElementNotFound("x".into());
        assert!(is_element_cdp_error(&not_found));
        assert!(!is_stale_node_error(&not_found));
    }

    #[test]
    fn test_missing_node_message_variants() {
        assert!(is_missing_node_message("Could not find node with given id"));
        assert!(is_missing_node_message("No node with given id found"));
        assert!(!is_missing_node_message("box model could not be computed"));
    }
}
