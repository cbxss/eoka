//! Page Abstraction
//!
//! High-level API for interacting with a browser page.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::cdp::types::{NetworkRequest, NetworkResponse};
use crate::cdp::{Cookie, MouseButton, MouseEventType, Session};
use crate::error::{Error, Result};
use crate::stealth::{Human, HumanSpeed};
use crate::StealthConfig;

/// Global counter for unique marker IDs to prevent race conditions
static MARKER_COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Handle the result of a try-click operation
fn handle_try_result(result: Result<()>) -> Result<bool> {
    match result {
        Ok(()) => Ok(true),
        Err(Error::Cdp { .. }) | Err(Error::ElementNotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Handle the result of a try-click operation that returns coordinates
fn handle_try_center_result(result: Result<(f64, f64)>) -> Result<Option<(f64, f64)>> {
    match result {
        Ok(coords) => Ok(Some(coords)),
        Err(Error::Cdp { .. }) | Err(Error::ElementNotFound(_)) => Ok(None),
        Err(e) => Err(e),
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
        let result = self
            .session
            .evaluate("document.documentElement.outerHTML")
            .await?;
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
        self.session
            .capture_screenshot(Some("jpeg"), Some(quality))
            .await
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

    // =========================================================================
    // Text-based Element Finding
    // =========================================================================

    /// Find an element by its text content
    ///
    /// Searches through common interactive elements (a, button, input, label, span, div, p)
    /// for text that contains the given string (case-insensitive).
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// let btn = page.find_by_text("Sign In").await?;
    /// btn.click().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn find_by_text(&self, text: &str) -> Result<Element<'_>> {
        self.find_by_text_match(text, TextMatch::Contains).await
    }

    /// Find an element by text with specific matching strategy
    ///
    /// Prioritizes interactive elements (a, button, input) over static elements.
    pub async fn find_by_text_match(
        &self,
        text: &str,
        match_type: TextMatch,
    ) -> Result<Element<'_>> {
        // Use unique marker ID to prevent race conditions between concurrent calls
        let marker_id = MARKER_COUNTER.fetch_add(1, Ordering::SeqCst);
        let marker_attr = format!("data-eoka-text-{}", marker_id);

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

        // Prioritize interactive elements first, then fall back to static elements
        let js = format!(
            r#"
            (() => {{
                // First pass: prioritize interactive elements (links, buttons, inputs)
                const interactive = 'a, button, input[type="submit"], input[type="button"], [role="button"], [onclick]';
                for (const el of document.querySelectorAll(interactive)) {{
                    const t = el.innerText || el.textContent || el.value || '';
                    if ({match_js}) {{
                        el.setAttribute('{marker_attr}', 'true');
                        return true;
                    }}
                }}

                // Second pass: other clickable/visible elements
                const secondary = 'label, span, div, p, h1, h2, h3, h4, h5, h6, li, td, th';
                for (const el of document.querySelectorAll(secondary)) {{
                    const t = el.innerText || el.textContent || el.value || '';
                    if ({match_js}) {{
                        el.setAttribute('{marker_attr}', 'true');
                        return true;
                    }}
                }}

                return false;
            }})()
            "#,
            match_js = match_js,
            marker_attr = marker_attr
        );

        let found: bool = self.evaluate(&js).await?;
        if !found {
            return Err(Error::ElementNotFound(format!("text: {}", text)));
        }

        // Now find it by the marker attribute
        let selector = format!("[{}='true']", marker_attr);
        let element = self.find(&selector).await?;

        // Clean up the marker
        let cleanup_js = format!(
            "document.querySelector('[{}]')?.removeAttribute('{}')",
            marker_attr, marker_attr
        );
        self.execute(&cleanup_js).await?;

        Ok(element)
    }

    /// Find all elements matching the given text
    pub async fn find_all_by_text(&self, text: &str) -> Result<Vec<Element<'_>>> {
        // Use unique marker ID to prevent race conditions
        let marker_id = MARKER_COUNTER.fetch_add(1, Ordering::SeqCst);
        let marker_attr = format!("data-eoka-text-{}", marker_id);

        let escaped_text = escape_js_string(text).to_lowercase();

        let js = format!(
            r#"
            (() => {{
                const selectors = 'a, button, input, label, span, div, p, h1, h2, h3, h4, h5, h6, li, td, th';
                const elements = document.querySelectorAll(selectors);
                let count = 0;
                for (const el of elements) {{
                    const t = (el.innerText || el.textContent || el.value || '').toLowerCase();
                    if (t.includes('{escaped_text}')) {{
                        el.setAttribute('{marker_attr}', count);
                        count++;
                    }}
                }}
                return count;
            }})()
            "#,
            escaped_text = escaped_text,
            marker_attr = marker_attr
        );

        let count: i32 = self.evaluate(&js).await?;
        let mut elements = Vec::new();

        // Collect elements, ensuring cleanup happens even on errors
        let result: Result<()> = async {
            for i in 0..count {
                if let Ok(el) = self.find(&format!("[{}='{}']", marker_attr, i)).await {
                    elements.push(el);
                }
            }
            Ok(())
        }
        .await;

        // Always clean up markers, even if collection failed
        let cleanup_js = format!(
            "document.querySelectorAll('[{}]').forEach(el => el.removeAttribute('{}'))",
            marker_attr, marker_attr
        );
        let _ = self.execute(&cleanup_js).await;

        result?;
        Ok(elements)
    }

    /// Check if an element with the given text exists
    #[must_use = "returns true if text exists on page"]
    pub async fn text_exists(&self, text: &str) -> bool {
        self.find_by_text(text).await.is_ok()
    }

    // =========================================================================
    // Interaction (Direct)
    // =========================================================================

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
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.click_by_text("Sign In").await?;
    /// page.click_by_text("Submit").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn click_by_text(&self, text: &str) -> Result<()> {
        let element = self.find_by_text(text).await?;
        element.click().await
    }

    /// Try to click an element, returning Ok(true) if clicked, Ok(false) if not found or not clickable
    ///
    /// Unlike `click()`, this doesn't error when the element is missing or not visible.
    /// Useful for optional UI elements like cookie banners or popups.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// // Dismiss cookie banner if present
    /// if page.try_click("button.accept-cookies").await? {
    ///     println!("Cookie banner dismissed");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_click(&self, selector: &str) -> Result<bool> {
        match self.find(selector).await {
            Ok(element) => handle_try_result(element.click().await),
            Err(Error::ElementNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Try to click an element by text, returning Ok(true) if clicked, Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_click_by_text(&self, text: &str) -> Result<bool> {
        match self.find_by_text(text).await {
            Ok(element) => handle_try_result(element.click().await),
            Err(Error::ElementNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Fill a form field: clicks, clears existing content, and types new value
    ///
    /// This is the recommended way to fill form fields as it handles
    /// clearing any existing content first.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.fill("#email", "user@example.com").await?;
    /// page.fill("#password", "secret123").await?;
    /// page.click("button[type='submit']").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fill(&self, selector: &str, value: &str) -> Result<()> {
        let element = self.find(selector).await?;
        element.click().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Select all and delete to clear the field
        self.execute("document.activeElement.select()").await?;
        self.session.insert_text("").await?;

        // Now type the new value
        self.session.insert_text(value).await
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
    pub async fn human_type_with_speed(
        &self,
        selector: &str,
        text: &str,
        speed: HumanSpeed,
    ) -> Result<()> {
        self.human_click_with_speed(selector, speed).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.human().with_speed(speed).type_text(text).await
    }

    /// Human-like click on an element found by text content
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.human_click_by_text("Sign In").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn human_click_by_text(&self, text: &str) -> Result<()> {
        let element = self.find_by_text(text).await?;
        let (x, y) = element.center().await?;

        if self.config.human_mouse {
            self.human().move_and_click(x, y).await
        } else {
            self.click_at(x, y).await
        }
    }

    /// Try to human-click an element, returning Ok(true) if clicked, Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_human_click(&self, selector: &str) -> Result<bool> {
        match self.find(selector).await {
            Ok(element) => {
                if let Some((x, y)) = handle_try_center_result(element.center().await)? {
                    if self.config.human_mouse {
                        self.human().move_and_click(x, y).await?;
                    } else {
                        self.click_at(x, y).await?;
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(Error::ElementNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Try to human-click an element by text, returning Ok(true) if clicked, Ok(false) if not found or not clickable
    #[must_use = "returns true if clicked, false if not found/visible"]
    pub async fn try_human_click_by_text(&self, text: &str) -> Result<bool> {
        match self.find_by_text(text).await {
            Ok(element) => {
                if let Some((x, y)) = handle_try_center_result(element.center().await)? {
                    if self.config.human_mouse {
                        self.human().move_and_click(x, y).await?;
                    } else {
                        self.click_at(x, y).await?;
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(Error::ElementNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Human-like form fill: clicks, clears, and types with natural delays
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.human_fill("#email", "user@example.com").await?;
    /// page.human_fill("#password", "secret123").await?;
    /// page.human_click_by_text("Sign In").await?;
    /// # Ok(())
    /// # }
    /// ```
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

    // =========================================================================
    // Wait Helpers
    // =========================================================================

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

    /// Wait for an element to be visible and clickable (has computable box model)
    ///
    /// Unlike `wait_for`, this ensures the element is actually rendered and
    /// can be interacted with, not just present in the DOM.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// // Wait for form to be fully rendered and clickable
    /// page.wait_for_visible("#email", 10_000).await?;
    /// page.human_fill("#email", "user@example.com").await?;
    /// # Ok(())
    /// # }
    /// ```
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
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.click_by_text("Submit").await?;
    /// page.wait_for_text("Success!", 10_000).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Wait for text to disappear from the page
    pub async fn wait_for_text_hidden(&self, text: &str, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if self.find_by_text(text).await.is_err() {
                return Ok(());
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "Element with text '{}' still visible after {}ms",
                    text, timeout_ms
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait for the URL to contain a specific string
    ///
    /// Useful after clicking navigation links to wait for the new page.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.human_click_by_text("Sign In").await?;
    /// page.wait_for_url_contains("login", 10_000).await?;
    /// # Ok(())
    /// # }
    /// ```
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
    ///
    /// Useful when you don't know exactly where a click will navigate.
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

    // =========================================================================
    // Selector Fallback Chains
    // =========================================================================

    /// Find the first element matching any of the given selectors
    ///
    /// Tries each selector in order and returns the first match.
    /// Useful when elements have inconsistent selectors across pages.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// let email = page.find_any(&["#email", "input[type='email']", "[name='email']"]).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Wait for any of the given selectors to be visible and clickable
    pub async fn wait_for_any_visible(
        &self,
        selectors: &[&str],
        timeout_ms: u64,
    ) -> Result<Element<'_>> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            for selector in selectors {
                if let Ok(element) = self.find(selector).await {
                    if element.is_visible().await.unwrap_or(false) {
                        return Ok(element);
                    }
                }
            }

            if start.elapsed() > timeout {
                return Err(Error::Timeout(format!(
                    "None of selectors visible within {}ms: {:?}",
                    timeout_ms, selectors
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // =========================================================================
    // Network Idle Waiting
    // =========================================================================

    /// Wait for network to become idle (no pending requests)
    ///
    /// Useful for pages that load content dynamically via JavaScript.
    /// Waits until there are no network requests for `idle_time_ms` milliseconds.
    ///
    /// NOTE: This requires network capture to be enabled and may be slightly
    /// detectable by advanced anti-bot systems.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.human_click_by_text("Load More").await?;
    /// page.wait_for_network_idle(500, 30_000).await?;  // 500ms idle, 30s timeout
    /// # Ok(())
    /// # }
    /// ```
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

    // =========================================================================
    // Frame/Iframe Support
    // =========================================================================

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

    // =========================================================================
    // Retry Wrapper
    // =========================================================================

    /// Retry an operation multiple times with delays between attempts
    ///
    /// Useful for flaky operations that may fail due to timing issues.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// page.with_retry(3, 500, || async {
    ///     page.human_click("#flaky-button").await
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    // =========================================================================
    // Debug Helpers
    // =========================================================================

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
        let url = self.url().await.unwrap_or_else(|_| "unknown".to_string());
        let title = self.title().await.unwrap_or_else(|_| "unknown".to_string());

        let input_count: u32 = self
            .evaluate("document.querySelectorAll('input').length")
            .await
            .unwrap_or(0);
        let button_count: u32 = self
            .evaluate("document.querySelectorAll('button').length")
            .await
            .unwrap_or(0);
        let link_count: u32 = self
            .evaluate("document.querySelectorAll('a').length")
            .await
            .unwrap_or(0);
        let form_count: u32 = self
            .evaluate("document.querySelectorAll('form').length")
            .await
            .unwrap_or(0);

        Ok(PageState {
            url,
            title,
            input_count,
            button_count,
            link_count,
            form_count,
        })
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
#[derive(Debug, Clone)]
pub struct PageState {
    /// Current URL
    pub url: String,
    /// Page title
    pub title: String,
    /// Number of input elements
    pub input_count: u32,
    /// Number of buttons
    pub button_count: u32,
    /// Number of links
    pub link_count: u32,
    /// Number of forms
    pub form_count: u32,
}

/// Bounding box of an element
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    /// X coordinate (left edge)
    pub x: f64,
    /// Y coordinate (top edge)
    pub y: f64,
    /// Width
    pub width: f64,
    /// Height
    pub height: f64,
}

impl BoundingBox {
    /// Get the center point
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
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

    /// Get inner text
    ///
    /// Extracts text content from the element's outerHTML without using focus.
    pub async fn text(&self) -> Result<String> {
        let value = self
            .eval_on_element("document.activeElement.textContent || ''")
            .await?;

        if let Some(s) = value.as_str() {
            return Ok(s.to_string());
        }
        Ok(String::new())
    }

    /// Helper to evaluate a JavaScript function on this element via Runtime.callFunctionOn
    ///
    /// The function receives `this` bound to the element. Write expressions as
    /// `function() { return this.tagName; }` style.
    async fn eval_on_element(&self, js_expr: &str) -> Result<serde_json::Value> {
        let object_id = self.page.session.resolve_node(self.node_id).await?;

        // Wrap the expression in a function that uses `this` instead of `document.activeElement`
        let func = format!(
            "function() {{ return {}; }}",
            js_expr.replace("document.activeElement", "this")
        );

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

    // =========================================================================
    // Element Inspection
    // =========================================================================

    /// Check if the element is visible (has a computable box model)
    ///
    /// Returns Ok(true) if the element is rendered and potentially visible,
    /// Ok(false) if the element exists but is not rendered (display:none, etc.),
    /// or Err if there was a network/session error.
    ///
    /// Note: This doesn't check CSS visibility or opacity, just box model existence.
    #[must_use = "returns visibility state"]
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

    /// Get an attribute value from the element
    ///
    /// # Example
    /// ```rust,no_run
    /// # use eoka::{Browser, Result};
    /// # async fn example() -> Result<()> {
    /// # let browser = Browser::launch().await?;
    /// # let page = browser.new_page("https://example.com").await?;
    /// let link = page.find("a.nav-link").await?;
    /// if let Some(href) = link.get_attribute("href").await? {
    ///     println!("Link goes to: {}", href);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_attribute(&self, name: &str) -> Result<Option<String>> {
        let escaped_name = escape_js_string(name);
        let value = self
            .eval_on_element(&format!(
                "document.activeElement.getAttribute('{}')",
                escaped_name
            ))
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
        let value = self
            .eval_on_element("document.activeElement.tagName.toLowerCase()")
            .await?;

        if let Some(s) = value.as_str() {
            return Ok(s.to_string());
        }
        Ok(String::new())
    }

    /// Check if the element is enabled (not disabled)
    pub async fn is_enabled(&self) -> Result<bool> {
        let value = self
            .eval_on_element("!document.activeElement.disabled")
            .await?;

        if let Some(b) = value.as_bool() {
            return Ok(b);
        }
        Ok(true) // Default to enabled if we can't determine
    }

    /// Check if a checkbox/radio is checked
    pub async fn is_checked(&self) -> Result<bool> {
        let value = self
            .eval_on_element("document.activeElement.checked === true")
            .await?;

        if let Some(b) = value.as_bool() {
            return Ok(b);
        }
        Ok(false)
    }

    /// Get the value of an input element
    pub async fn value(&self) -> Result<String> {
        let value = self
            .eval_on_element("document.activeElement.value || ''")
            .await?;

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
                "getComputedStyle(document.activeElement).getPropertyValue('{}')",
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
