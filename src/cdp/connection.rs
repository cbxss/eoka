//! CDP Connection/Session Management
//!
//! Manages browser and page sessions over the CDP transport.

use std::sync::Arc;

use super::transport::Transport;
use super::types::*;
use crate::error::Result;

/// A CDP connection to Chrome
pub struct Connection {
    transport: Arc<Transport>,
}

impl Connection {
    /// Create a new connection wrapping a transport
    pub fn new(transport: Transport) -> Self {
        Self {
            transport: Arc::new(transport),
        }
    }

    /// Get a reference to the transport
    pub fn transport(&self) -> &Arc<Transport> {
        &self.transport
    }

    /// Get browser version info
    pub async fn version(&self) -> Result<BrowserGetVersionResult> {
        self.transport
            .send("Browser.getVersion", &BrowserGetVersion {})
            .await
    }

    /// Create a new target (tab)
    pub async fn create_target(
        &self,
        url: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<String> {
        let result: TargetCreateTargetResult = self
            .transport
            .send(
                "Target.createTarget",
                &TargetCreateTarget {
                    url: url.to_string(),
                    width,
                    height,
                },
            )
            .await?;
        Ok(result.target_id)
    }

    /// Attach to a target and get a session
    pub async fn attach_to_target(&self, target_id: &str) -> Result<Session> {
        let result: TargetAttachToTargetResult = self
            .transport
            .send(
                "Target.attachToTarget",
                &TargetAttachToTarget {
                    target_id: target_id.to_string(),
                    flatten: Some(true),
                },
            )
            .await?;

        Ok(Session {
            transport: Arc::clone(&self.transport),
            session_id: result.session_id,
            target_id: target_id.to_string(),
        })
    }

    /// Close a target
    pub async fn close_target(&self, target_id: &str) -> Result<bool> {
        let result: TargetCloseTargetResult = self
            .transport
            .send(
                "Target.closeTarget",
                &TargetCloseTarget {
                    target_id: target_id.to_string(),
                },
            )
            .await?;
        Ok(result.success)
    }

    /// Close the browser
    pub async fn close(&self) -> Result<()> {
        let _ = self
            .transport
            .send::<_, serde_json::Value>("Browser.close", &BrowserClose {})
            .await;
        self.transport.close().await
    }
}

/// A CDP session attached to a specific target
pub struct Session {
    transport: Arc<Transport>,
    session_id: String,
    target_id: String,
}

impl Session {
    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the target ID
    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    /// Send a command to this session
    pub async fn send<C, R>(&self, method: &str, params: &C) -> Result<R>
    where
        C: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        self.transport
            .send_to_session(&self.session_id, method, params)
            .await
    }

    /// Enable page events
    pub async fn page_enable(&self) -> Result<()> {
        self.send::<_, serde_json::Value>("Page.enable", &PageEnable {})
            .await?;
        Ok(())
    }

    /// Navigate to a URL
    pub async fn navigate(&self, url: &str) -> Result<PageNavigateResult> {
        self.send(
            "Page.navigate",
            &PageNavigate {
                url: url.to_string(),
            },
        )
        .await
    }

    /// Reload the page
    pub async fn reload(&self, ignore_cache: bool) -> Result<()> {
        self.send::<_, serde_json::Value>(
            "Page.reload",
            &PageReload {
                ignore_cache: Some(ignore_cache),
                script_to_evaluate_on_load: None,
            },
        )
        .await?;
        Ok(())
    }

    /// Go back in history
    pub async fn go_back(&self) -> Result<()> {
        let history: PageGetNavigationHistoryResult = self
            .send("Page.getNavigationHistory", &PageGetNavigationHistory {})
            .await?;
        if history.current_index > 0 {
            let entry_id = history.entries[history.current_index as usize - 1].id;
            self.send::<_, serde_json::Value>(
                "Page.navigateToHistoryEntry",
                &PageNavigateToHistoryEntry { entry_id },
            )
            .await?;
        }
        Ok(())
    }

    /// Go forward in history
    pub async fn go_forward(&self) -> Result<()> {
        let history: PageGetNavigationHistoryResult = self
            .send("Page.getNavigationHistory", &PageGetNavigationHistory {})
            .await?;
        let next = history.current_index as usize + 1;
        if next < history.entries.len() {
            let entry_id = history.entries[next].id;
            self.send::<_, serde_json::Value>(
                "Page.navigateToHistoryEntry",
                &PageNavigateToHistoryEntry { entry_id },
            )
            .await?;
        }
        Ok(())
    }

    /// Add a script to evaluate on every new document
    pub async fn add_script_to_evaluate_on_new_document(&self, source: &str) -> Result<String> {
        let result: PageAddScriptToEvaluateOnNewDocumentResult = self
            .send(
                "Page.addScriptToEvaluateOnNewDocument",
                &PageAddScriptToEvaluateOnNewDocument {
                    source: source.to_string(),
                    world_name: None,
                    include_command_line_api: None,
                },
            )
            .await?;
        Ok(result.identifier)
    }

    /// Capture a screenshot
    pub async fn capture_screenshot(
        &self,
        format: Option<&str>,
        quality: Option<u8>,
    ) -> Result<Vec<u8>> {
        let result: PageCaptureScreenshotResult = self
            .send(
                "Page.captureScreenshot",
                &PageCaptureScreenshot {
                    format: format.map(String::from),
                    quality,
                },
            )
            .await?;

        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&result.data)
            .map_err(|e| crate::error::Error::Decode(e.to_string()))?;
        Ok(bytes)
    }

    /// Get the frame tree
    pub async fn get_frame_tree(&self) -> Result<FrameTree> {
        let result: PageGetFrameTreeResult =
            self.send("Page.getFrameTree", &PageGetFrameTree {}).await?;
        Ok(result.frame_tree)
    }

    /// Dispatch a mouse event (click, move, or wheel)
    pub async fn dispatch_mouse_event(
        &self,
        event_type: MouseEventType,
        x: f64,
        y: f64,
        button: Option<MouseButton>,
        click_count: Option<i32>,
    ) -> Result<()> {
        self.dispatch_mouse_event_full(InputDispatchMouseEvent {
            r#type: event_type,
            x,
            y,
            button,
            click_count,
            delta_x: None,
            delta_y: None,
        })
        .await
    }

    /// Dispatch a mouse wheel scroll event
    pub async fn dispatch_mouse_wheel(
        &self,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<()> {
        self.dispatch_mouse_event_full(InputDispatchMouseEvent {
            r#type: MouseEventType::MouseWheel,
            x,
            y,
            button: None,
            click_count: None,
            delta_x: Some(delta_x),
            delta_y: Some(delta_y),
        })
        .await
    }

    /// Dispatch a raw mouse event with full control over all fields
    pub async fn dispatch_mouse_event_full(&self, event: InputDispatchMouseEvent) -> Result<()> {
        self.send::<_, serde_json::Value>("Input.dispatchMouseEvent", &event)
            .await?;
        Ok(())
    }

    /// Dispatch a key event
    pub async fn dispatch_key_event(
        &self,
        event_type: KeyEventType,
        key: Option<&str>,
        text: Option<&str>,
        code: Option<&str>,
    ) -> Result<()> {
        self.send::<_, serde_json::Value>(
            "Input.dispatchKeyEvent",
            &InputDispatchKeyEvent {
                r#type: event_type,
                text: text.map(String::from),
                code: code.map(String::from),
                key: key.map(String::from),
            },
        )
        .await?;
        Ok(())
    }

    /// Insert text at current cursor position
    pub async fn insert_text(&self, text: &str) -> Result<()> {
        self.send::<_, serde_json::Value>(
            "Input.insertText",
            &InputInsertText {
                text: text.to_string(),
            },
        )
        .await?;
        Ok(())
    }

    /// Get the document root node
    pub async fn get_document(&self, depth: Option<i32>) -> Result<DOMNode> {
        let result: DOMGetDocumentResult = self
            .send(
                "DOM.getDocument",
                &DOMGetDocument {
                    depth,
                    pierce: Some(true),
                },
            )
            .await?;
        Ok(result.root)
    }

    /// Query for a single element
    pub async fn query_selector(&self, node_id: i32, selector: &str) -> Result<i32> {
        let result: DOMQuerySelectorResult = self
            .send(
                "DOM.querySelector",
                &DOMQuerySelector {
                    node_id,
                    selector: selector.to_string(),
                },
            )
            .await?;
        Ok(result.node_id)
    }

    /// Query for all matching elements
    pub async fn query_selector_all(&self, node_id: i32, selector: &str) -> Result<Vec<i32>> {
        let result: DOMQuerySelectorAllResult = self
            .send(
                "DOM.querySelectorAll",
                &DOMQuerySelectorAll {
                    node_id,
                    selector: selector.to_string(),
                },
            )
            .await?;
        Ok(result.node_ids)
    }

    /// Get the box model for an element
    pub async fn get_box_model(&self, node_id: i32) -> Result<BoxModel> {
        let result: DOMGetBoxModelResult = self
            .send(
                "DOM.getBoxModel",
                &DOMGetBoxModel {
                    node_id: Some(node_id),
                },
            )
            .await?;
        Ok(result.model)
    }

    /// Get outer HTML of an element
    pub async fn get_outer_html(&self, node_id: i32) -> Result<String> {
        let result: DOMGetOuterHTMLResult = self
            .send(
                "DOM.getOuterHTML",
                &DOMGetOuterHTML {
                    node_id: Some(node_id),
                },
            )
            .await?;
        Ok(result.outer_html)
    }

    /// Resolve a DOM node to a Runtime remote object ID
    pub async fn resolve_node(&self, node_id: i32) -> Result<String> {
        let result: DOMResolveNodeResult = self
            .send(
                "DOM.resolveNode",
                &DOMResolveNode {
                    node_id: Some(node_id),
                    object_group: Some("eoka".to_string()),
                },
            )
            .await?;
        result
            .object
            .object_id
            .ok_or_else(|| crate::error::Error::Cdp {
                method: "DOM.resolveNode".to_string(),
                code: -1,
                message: "No object_id returned".to_string(),
            })
    }

    /// Call a function on a remote object and return the result by value
    pub async fn call_function_on(
        &self,
        object_id: &str,
        function_declaration: &str,
    ) -> Result<RuntimeEvaluateResult> {
        self.call_function_on_impl(object_id, function_declaration, true)
            .await
    }

    /// Focus an element
    pub async fn focus(&self, node_id: i32) -> Result<()> {
        self.send::<_, serde_json::Value>(
            "DOM.focus",
            &DOMFocus {
                node_id: Some(node_id),
            },
        )
        .await?;
        Ok(())
    }

    /// Get all cookies
    pub async fn get_cookies(&self, urls: Option<Vec<String>>) -> Result<Vec<Cookie>> {
        let result: NetworkGetCookiesResult = self
            .send("Network.getCookies", &NetworkGetCookies { urls })
            .await?;
        Ok(result.cookies)
    }

    /// Set a cookie
    pub async fn set_cookie(
        &self,
        name: &str,
        value: &str,
        url: Option<&str>,
        domain: Option<&str>,
        path: Option<&str>,
    ) -> Result<bool> {
        let result: NetworkSetCookieResult = self
            .send(
                "Network.setCookie",
                &NetworkSetCookie {
                    name: name.to_string(),
                    value: value.to_string(),
                    url: url.map(String::from),
                    domain: domain.map(String::from),
                    path: path.map(String::from),
                    ..Default::default()
                },
            )
            .await?;
        Ok(result.success)
    }

    /// Delete cookies
    pub async fn delete_cookies(
        &self,
        name: &str,
        url: Option<&str>,
        domain: Option<&str>,
    ) -> Result<()> {
        self.send::<_, serde_json::Value>(
            "Network.deleteCookies",
            &NetworkDeleteCookies {
                name: name.to_string(),
                url: url.map(String::from),
                domain: domain.map(String::from),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    /// Enable network events (request/response capture)
    /// NOTE: This enables Network.enable which may be slightly detectable
    pub async fn network_enable(&self) -> Result<()> {
        self.send::<_, serde_json::Value>(
            "Network.enable",
            &NetworkEnable {
                max_post_data_size: Some(65536), // Capture POST data up to 64KB
            },
        )
        .await?;
        Ok(())
    }

    /// Disable network events
    pub async fn network_disable(&self) -> Result<()> {
        self.send::<_, serde_json::Value>("Network.disable", &NetworkDisable {})
            .await?;
        Ok(())
    }

    /// Get response body for a request
    pub async fn get_response_body(&self, request_id: &str) -> Result<(String, bool)> {
        let result: NetworkGetResponseBodyResult = self
            .send(
                "Network.getResponseBody",
                &NetworkGetResponseBody {
                    request_id: request_id.to_string(),
                },
            )
            .await?;
        Ok((result.body, result.base64_encoded))
    }

    /// Evaluate JavaScript and return a remote object reference (not by value).
    pub async fn evaluate_for_remote_object(
        &self,
        expression: &str,
    ) -> Result<RuntimeEvaluateResult> {
        self.evaluate_impl(expression, false, Some("eoka")).await
    }

    /// Convert a remote object ID to a DOM node_id via DOM.requestNode
    pub async fn request_node(&self, object_id: &str) -> Result<i32> {
        let result: DOMRequestNodeResult = self
            .send(
                "DOM.requestNode",
                &DOMRequestNode {
                    object_id: object_id.to_string(),
                },
            )
            .await?;
        Ok(result.node_id)
    }

    /// Get all own properties of a remote object (used for array element enumeration)
    pub async fn get_properties(
        &self,
        object_id: &str,
    ) -> Result<Vec<crate::cdp::types::PropertyDescriptor>> {
        let result: crate::cdp::types::RuntimeGetPropertiesResult = self
            .send(
                "Runtime.getProperties",
                &crate::cdp::types::RuntimeGetProperties {
                    object_id: object_id.to_string(),
                    own_properties: Some(true),
                },
            )
            .await?;
        Ok(result.result)
    }

    async fn call_function_on_impl(
        &self,
        object_id: &str,
        function_declaration: &str,
        return_by_value: bool,
    ) -> Result<RuntimeEvaluateResult> {
        let result: RuntimeCallFunctionOnResult = self
            .send(
                "Runtime.callFunctionOn",
                &RuntimeCallFunctionOn {
                    function_declaration: function_declaration.to_string(),
                    object_id: Some(object_id.to_string()),
                    arguments: None,
                    silent: None,
                    return_by_value: Some(return_by_value),
                    await_promise: Some(true),
                },
            )
            .await?;
        Ok(RuntimeEvaluateResult {
            result: result.result,
            exception_details: result.exception_details,
        })
    }

    /// Evaluate JavaScript expression and return the result by value
    pub async fn evaluate(&self, expression: &str) -> Result<RuntimeEvaluateResult> {
        self.evaluate_impl(expression, true, None).await
    }

    async fn evaluate_impl(
        &self,
        expression: &str,
        return_by_value: bool,
        object_group: Option<&str>,
    ) -> Result<RuntimeEvaluateResult> {
        self.send(
            "Runtime.evaluate",
            &RuntimeEvaluate {
                expression: expression.to_string(),
                object_group: object_group.map(String::from),
                return_by_value: Some(return_by_value),
                await_promise: Some(true),
            },
        )
        .await
    }
}
