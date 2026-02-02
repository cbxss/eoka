//! Hand-written CDP types for the ~20 commands we actually use
//!
//! These replace the massive chromiumoxide-generated types with a minimal set
//! that's just enough for stealth browser automation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Target Domain - Tab/Target Management
// ============================================================================

/// Create a new browser target (tab)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCreateTarget {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_window: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCreateTargetResult {
    #[serde(default)]
    pub target_id: String,
}

/// Close a target
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCloseTarget {
    pub target_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TargetCloseTargetResult {
    #[serde(default)]
    pub success: bool,
}

/// Attach to a target for debugging
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetAttachToTarget {
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flatten: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetAttachToTargetResult {
    #[serde(default)]
    pub session_id: String,
}

/// Get list of available targets
#[derive(Debug, Clone, Default, Serialize)]
pub struct TargetGetTargets {}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetGetTargetsResult {
    #[serde(default)]
    pub target_infos: Vec<TargetInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub target_id: String,
    pub r#type: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub attached: bool,
    #[serde(default)]
    pub opener_id: Option<String>,
    #[serde(default)]
    pub browser_context_id: Option<String>,
}

/// Set discover targets to receive target events
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetSetDiscoverTargets {
    pub discover: bool,
}

// ============================================================================
// Page Domain - Navigation and Scripts
// ============================================================================

/// Navigate to URL
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageNavigate {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageNavigateResult {
    #[serde(default)]
    pub frame_id: String,
    #[serde(default)]
    pub loader_id: Option<String>,
    #[serde(default)]
    pub error_text: Option<String>,
}

/// Enable page events
#[derive(Debug, Clone, Default, Serialize)]
pub struct PageEnable {}

/// Reload the page
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageReload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_cache: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_to_evaluate_on_load: Option<String>,
}

/// Get navigation history
#[derive(Debug, Clone, Default, Serialize)]
pub struct PageGetNavigationHistory {}

/// Navigation history result
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGetNavigationHistoryResult {
    pub current_index: i32,
    pub entries: Vec<NavigationEntry>,
}

/// A navigation history entry
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEntry {
    pub id: i32,
    pub url: String,
    pub user_typed_url: Option<String>,
    pub title: String,
}

/// Navigate to a specific history entry
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageNavigateToHistoryEntry {
    pub entry_id: i32,
}

/// Add script to evaluate on new document
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageAddScriptToEvaluateOnNewDocument {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_command_line_api: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageAddScriptToEvaluateOnNewDocumentResult {
    #[serde(default)]
    pub identifier: String,
}

/// Bypass Content Security Policy
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSetBypassCSP {
    pub enabled: bool,
}

/// Capture screenshot
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCaptureScreenshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>, // "png" | "jpeg" | "webp"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<Viewport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_surface: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_beyond_viewport: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageCaptureScreenshotResult {
    #[serde(default)]
    pub data: String, // base64 encoded
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

/// Get frame tree
#[derive(Debug, Clone, Default, Serialize)]
pub struct PageGetFrameTree {}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGetFrameTreeResult {
    #[serde(default)]
    pub frame_tree: FrameTree,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameTree {
    #[serde(default)]
    pub frame: Frame,
    #[serde(default)]
    pub child_frames: Vec<FrameTree>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub loader_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub security_origin: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

// ============================================================================
// Input Domain - Mouse and Keyboard Events
// ============================================================================

/// Dispatch a mouse event
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDispatchMouseEvent {
    pub r#type: MouseEventType,
    pub x: f64,
    pub y: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button: Option<MouseButton>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub click_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer_type: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseEventType {
    MousePressed,
    MouseReleased,
    MouseMoved,
    MouseWheel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    None,
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

/// Dispatch a key event
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDispatchKeyEvent {
    pub r#type: KeyEventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmodified_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_virtual_key_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_virtual_key_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_repeat: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_keypad: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_system_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyEventType {
    KeyDown,
    KeyUp,
    RawKeyDown,
    Char,
}

/// Insert text at current cursor position
#[derive(Debug, Clone, Serialize)]
pub struct InputInsertText {
    pub text: String,
}

// ============================================================================
// Network Domain - Cookies
// ============================================================================

/// Get cookies
#[derive(Debug, Clone, Default, Serialize)]
pub struct NetworkGetCookies {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urls: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NetworkGetCookiesResult {
    #[serde(default)]
    pub cookies: Vec<Cookie>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub size: i32,
    pub http_only: bool,
    pub secure: bool,
    pub session: bool,
    #[serde(default)]
    pub same_site: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub same_party: Option<bool>,
    #[serde(default)]
    pub source_scheme: Option<String>,
    #[serde(default)]
    pub source_port: Option<i32>,
}

/// Set a cookie
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSetCookie {
    pub name: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NetworkSetCookieResult {
    #[serde(default)]
    pub success: bool,
}

/// Delete cookies
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDeleteCookies {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Enable network events
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEnable {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_total_buffer_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_resource_buffer_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_post_data_size: Option<i64>,
}

/// Disable network events
#[derive(Debug, Clone, Default, Serialize)]
pub struct NetworkDisable {}

/// Get response body
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkGetResponseBody {
    pub request_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkGetResponseBodyResult {
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub base64_encoded: bool,
}

/// HTTP request info
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRequest {
    pub url: String,
    #[serde(default)]
    pub url_fragment: Option<String>,
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub post_data: Option<String>,
    #[serde(default)]
    pub has_post_data: Option<bool>,
    #[serde(default)]
    pub mixed_content_type: Option<String>,
    #[serde(default)]
    pub initial_priority: Option<String>,
    #[serde(default)]
    pub referrer_policy: Option<String>,
}

/// HTTP response info
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResponse {
    pub url: String,
    pub status: i32,
    pub status_text: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub request_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub connection_reused: Option<bool>,
    #[serde(default)]
    pub connection_id: Option<f64>,
    #[serde(default)]
    pub remote_ip_address: Option<String>,
    #[serde(default)]
    pub remote_port: Option<i32>,
    #[serde(default)]
    pub from_disk_cache: Option<bool>,
    #[serde(default)]
    pub from_service_worker: Option<bool>,
    #[serde(default)]
    pub from_prefetch_cache: Option<bool>,
    #[serde(default)]
    pub encoded_data_length: Option<i64>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub security_state: Option<String>,
}

/// Network.requestWillBeSent event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRequestWillBeSentEvent {
    pub request_id: String,
    pub loader_id: String,
    pub document_url: String,
    pub request: NetworkRequest,
    pub timestamp: f64,
    pub wall_time: f64,
    #[serde(default)]
    pub initiator: Option<serde_json::Value>,
    #[serde(default)]
    pub redirect_has_extra_info: Option<bool>,
    #[serde(default)]
    pub redirect_response: Option<NetworkResponse>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub has_user_gesture: Option<bool>,
}

/// Network.responseReceived event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResponseReceivedEvent {
    pub request_id: String,
    pub loader_id: String,
    pub timestamp: f64,
    pub r#type: String,
    pub response: NetworkResponse,
    #[serde(default)]
    pub has_extra_info: Option<bool>,
    #[serde(default)]
    pub frame_id: Option<String>,
}

/// Network.loadingFinished event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLoadingFinishedEvent {
    pub request_id: String,
    pub timestamp: f64,
    pub encoded_data_length: i64,
}

/// Network.loadingFailed event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLoadingFailedEvent {
    pub request_id: String,
    pub timestamp: f64,
    pub r#type: String,
    pub error_text: String,
    #[serde(default)]
    pub canceled: Option<bool>,
    #[serde(default)]
    pub blocked_reason: Option<String>,
}

// ============================================================================
// DOM Domain - Element Finding (no Runtime.evaluate needed)
// ============================================================================

/// Get the document root node
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMGetDocument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pierce: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DOMGetDocumentResult {
    #[serde(default)]
    pub root: DOMNode,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMNode {
    #[serde(default)]
    pub node_id: i32,
    #[serde(default)]
    pub parent_id: Option<i32>,
    #[serde(default)]
    pub backend_node_id: i32,
    #[serde(default)]
    pub node_type: i32,
    #[serde(default)]
    pub node_name: String,
    #[serde(default)]
    pub local_name: String,
    #[serde(default)]
    pub node_value: String,
    #[serde(default)]
    pub child_node_count: Option<i32>,
    #[serde(default)]
    pub children: Option<Vec<DOMNode>>,
    #[serde(default)]
    pub attributes: Option<Vec<String>>,
    #[serde(default)]
    pub document_url: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub content_document: Option<Box<DOMNode>>,
}

/// Query selector
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMQuerySelector {
    pub node_id: i32,
    pub selector: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMQuerySelectorResult {
    #[serde(default)]
    pub node_id: i32,
}

/// Query selector all
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMQuerySelectorAll {
    pub node_id: i32,
    pub selector: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMQuerySelectorAllResult {
    #[serde(default)]
    pub node_ids: Vec<i32>,
}

/// Get box model for element (to get click coordinates)
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMGetBoxModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DOMGetBoxModelResult {
    #[serde(default)]
    pub model: BoxModel,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxModel {
    #[serde(default)]
    pub content: Vec<f64>, // [x1, y1, x2, y2, x3, y3, x4, y4] - quad
    #[serde(default)]
    pub padding: Vec<f64>,
    #[serde(default)]
    pub border: Vec<f64>,
    #[serde(default)]
    pub margin: Vec<f64>,
    #[serde(default)]
    pub width: i32,
    #[serde(default)]
    pub height: i32,
    #[serde(default)]
    pub shape_outside: Option<ShapeOutsideInfo>,
}

impl BoxModel {
    /// Get the center point of the content box
    pub fn center(&self) -> (f64, f64) {
        // Content is a quad: [x1, y1, x2, y2, x3, y3, x4, y4]
        // Center is average of the 4 corners
        if self.content.len() >= 8 {
            let x = (self.content[0] + self.content[2] + self.content[4] + self.content[6]) / 4.0;
            let y = (self.content[1] + self.content[3] + self.content[5] + self.content[7]) / 4.0;
            (x, y)
        } else {
            (0.0, 0.0)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeOutsideInfo {
    pub bounds: Vec<f64>,
    #[serde(default)]
    pub shape: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub margin_shape: Option<Vec<serde_json::Value>>,
}

/// Describe a node
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMDescribeNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pierce: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DOMDescribeNodeResult {
    #[serde(default)]
    pub node: DOMNode,
}

/// Get outer HTML
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMGetOuterHTML {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMGetOuterHTMLResult {
    #[serde(default)]
    pub outer_html: String,
}

/// Focus an element
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMFocus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
}

// ============================================================================
// Runtime Domain - MINIMAL (only what we can't avoid)
// Note: We avoid Runtime.enable as it's detectable. Use DOM methods instead.
// ============================================================================

/// Evaluate JavaScript (use sparingly - prefer DOM methods)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvaluate {
    pub expression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_command_line_api: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_by_value: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_preview: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_gesture: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub await_promise: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throw_on_side_effect: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_breaks: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repl_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unsafe_eval_blocked_by_csp: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvaluateResult {
    #[serde(default)]
    pub result: RemoteObject,
    #[serde(default)]
    pub exception_details: Option<ExceptionDetails>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteObject {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub class_name: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub unserializable_value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub object_id: Option<String>,
    #[serde(default)]
    pub preview: Option<ObjectPreview>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPreview {
    pub r#type: String,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub overflow: bool,
    pub properties: Vec<PropertyPreview>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyPreview {
    pub name: String,
    pub r#type: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionDetails {
    pub exception_id: i32,
    pub text: String,
    pub line_number: i32,
    pub column_number: i32,
    #[serde(default)]
    pub script_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub exception: Option<RemoteObject>,
    #[serde(default)]
    pub execution_context_id: Option<i32>,
}

/// Resolve a DOM node to a Runtime remote object
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMResolveNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_group: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMResolveNodeResult {
    #[serde(default)]
    pub object: RemoteObject,
}

/// Call a function on a remote object
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallFunctionOn {
    pub function_declaration: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<CallArgument>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_by_value: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub await_promise: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallArgument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallFunctionOnResult {
    #[serde(default)]
    pub result: RemoteObject,
    #[serde(default)]
    pub exception_details: Option<ExceptionDetails>,
}

// ============================================================================
// Browser Domain - Basic browser info
// ============================================================================

/// Get browser version info
#[derive(Debug, Clone, Default, Serialize)]
pub struct BrowserGetVersion {}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserGetVersionResult {
    #[serde(default)]
    pub protocol_version: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub js_version: String,
}

/// Close the browser
#[derive(Debug, Clone, Default, Serialize)]
pub struct BrowserClose {}

// ============================================================================
// Events
// ============================================================================

/// Page frame navigated event
#[derive(Debug, Clone, Deserialize)]
pub struct PageFrameNavigatedEvent {
    pub frame: Frame,
    #[serde(default)]
    pub r#type: Option<String>,
}

/// Page load event fired
#[derive(Debug, Clone, Deserialize)]
pub struct PageLoadEventFiredEvent {
    pub timestamp: f64,
}

/// Page DOM content loaded
#[derive(Debug, Clone, Deserialize)]
pub struct PageDomContentEventFiredEvent {
    pub timestamp: f64,
}

/// Target attached to target event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetAttachedToTargetEvent {
    pub session_id: String,
    pub target_info: TargetInfo,
    pub waiting_for_debugger: bool,
}

/// Target info changed event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetTargetInfoChangedEvent {
    pub target_info: TargetInfo,
}

/// Target destroyed event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetTargetDestroyedEvent {
    pub target_id: String,
}
