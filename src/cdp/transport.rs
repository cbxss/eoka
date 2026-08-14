//! CDP Transport Layer
//!
//! Handles communication with Chrome via WebSocket (tokio-tungstenite).
//! Includes built-in filtering to block detectable CDP commands.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::error::{Error, Result};

/// The connected WebSocket stream and its split write/read halves.
type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = SplitSink<WsStream, Message>;
type WsSource = SplitStream<WsStream>;

/// Shared write half — used by both `send_impl` and the reader task
/// (proxy-auth replies, pong frames).
type SharedSink = Arc<Mutex<WsSink>>;

/// Pending requests map — accessed from both the async reader task and
/// `send_impl`; the lock is held very briefly so a std Mutex is fine.
type PendingMap = std::sync::Mutex<HashMap<u64, PendingRequest>>;

/// A pending request waiting for a response
type PendingRequest = oneshot::Sender<Result<Value>>;

/// Recover a poisoned pending-request map. Its critical sections only mutate
/// `HashMap` entries, whose memory invariants remain intact during unwinding.
fn lock_pending(pending: &PendingMap) -> std::sync::MutexGuard<'_, HashMap<u64, PendingRequest>> {
    match pending.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::error!("Pending CDP request map was poisoned; recovering its contents");
            poisoned.into_inner()
        }
    }
}

/// Fail and remove every pending request, returning the number drained.
fn fail_pending(pending: &PendingMap, context: &str) -> usize {
    let mut requests = lock_pending(pending);
    let count = requests.len();
    for (_id, sender) in requests.drain() {
        drop(sender.send(Err(Error::transport(context))));
    }
    count
}

/// Terminate and reap a managed Chrome child without treating an already
/// exited process as an error.
fn stop_child(child: &mut Child) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    if let Err(kill_error) = child.kill() {
        if child.try_wait()?.is_none() {
            return Err(kill_error);
        }
        return Ok(());
    }

    child.wait()?;
    Ok(())
}

/// Own a Chrome child until it is handed to `Transport`. If the async
/// WebSocket connection future is cancelled, dropping this guard still kills
/// and reaps the process.
struct ChildCleanupGuard {
    child: Option<Child>,
}

impl ChildCleanupGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn take(&mut self) -> Result<Child> {
        self.child
            .take()
            .ok_or_else(|| Error::transport("Chrome child ownership was already transferred"))
    }
}

impl Drop for ChildCleanupGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(error) = stop_child(&mut child) {
                tracing::warn!("Failed to clean up an unclaimed Chrome child: {}", error);
            }
        }
    }
}

/// Broadcast capacity for CDP events (multi-consumer, lag observable).
const EVENT_CHANNEL_CAP: usize = 1024;

/// Check if a command should be blocked (highly detectable by anti-bot)
fn is_blocked(method: &str) -> bool {
    matches!(
        method,
        "Runtime.enable"
            | "Runtime.disable"
            | "HeapProfiler.enable"
            | "HeapProfiler.disable"
            | "Profiler.enable"
            | "Profiler.disable"
            | "Debugger.enable"
            | "Debugger.disable"
            | "Console.enable"
            | "Console.disable"
    )
}

/// Check if a command is risky (potentially detectable)
fn is_risky(method: &str) -> bool {
    matches!(
        method,
        "Emulation.setUserAgentOverride"
            | "Emulation.setTimezoneOverride"
            | "Emulation.setDeviceMetricsOverride"
            | "Page.setBypassCSP"
    )
}

/// CDP Transport - handles sending commands and receiving responses via WebSocket
pub struct Transport {
    /// The Chrome child process (None when connecting to an existing instance)
    child: Option<Mutex<Child>>,
    /// WebSocket write half (shared with the reader task for auth/pong).
    writer: SharedSink,
    /// Next message ID
    next_id: AtomicU64,
    /// Pending requests waiting for responses
    pending: Arc<PendingMap>,
    /// Broadcasts parsed events to all subscribers.
    event_tx: broadcast::Sender<CdpMessage>,
    /// Persistent receiver backing `recv_event`/`try_recv_event` (back-compat).
    event_rx: Mutex<broadcast::Receiver<CdpMessage>>,
    /// Timeout for CDP commands
    cmd_timeout: std::time::Duration,
    /// Reader task handle — checked for exit before sending commands.
    reader_handle: Option<tokio::task::JoinHandle<()>>,
    /// When true, drop "detectable" commands (`Runtime.enable`, etc.) silently.
    /// Defaults to true. Disable when you own the browser session and need
    /// full DevTools-equivalent control.
    filter_cdp: bool,
}

/// A parsed CDP message (response or event)
#[derive(Debug, Clone)]
pub enum CdpMessage {
    /// Response to a previously issued command.
    Response {
        /// Command ID this response corresponds to.
        id: u64,
        /// Command result, or the CDP error it returned.
        result: Result<Value>,
    },
    /// An event emitted by Chrome.
    Event {
        /// Event method name (e.g. "Network.requestWillBeSent").
        method: String,
        /// Event payload.
        params: Value,
        /// Session ID the event originated from, if any.
        session_id: Option<String>,
    },
}

impl Transport {
    /// Create a new transport connecting to Chrome via WebSocket
    pub async fn new(child: Child, ws_url: &str) -> Result<Self> {
        Self::new_with_options(child, ws_url, None, 30).await
    }

    /// Connect the WebSocket and return the stream.
    async fn ws_connect(ws_url: &str) -> Result<WsStream> {
        if !ws_url.starts_with("ws://") {
            return Err(Error::transport(format!(
                "Invalid WebSocket URL (expected ws://...): {}",
                ws_url
            )));
        }
        let (stream, _resp) = connect_async(ws_url)
            .await
            .map_err(|e| Error::transport(format!("WebSocket connect failed: {}", e)))?;
        tracing::debug!("WebSocket connected to {}", ws_url);
        Ok(stream)
    }

    /// Spawn the reader task and build the Transport from a connected stream.
    fn build(
        child: Option<Child>,
        stream: WsStream,
        proxy_auth: Option<(String, String)>,
        cdp_timeout_secs: u64,
        filter_cdp: bool,
    ) -> Self {
        let cmd_timeout = std::time::Duration::from_secs(cdp_timeout_secs);
        tracing::debug!("CDP timeout set to {}s", cdp_timeout_secs);

        let (sink, source) = stream.split();
        let writer: SharedSink = Arc::new(Mutex::new(sink));

        let pending: Arc<PendingMap> = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = broadcast::channel(EVENT_CHANNEL_CAP);

        let pending_clone = Arc::clone(&pending);
        let event_tx_clone = event_tx.clone();
        let reader_writer = Arc::clone(&writer);
        let reader_handle = tokio::spawn(async move {
            Self::reader_loop(
                source,
                pending_clone,
                event_tx_clone,
                reader_writer,
                proxy_auth,
            )
            .await;
        });

        Self {
            child: child.map(Mutex::new),
            writer,
            next_id: AtomicU64::new(1),
            pending,
            event_tx,
            event_rx: Mutex::new(event_rx),
            cmd_timeout,
            reader_handle: Some(reader_handle),
            filter_cdp,
        }
    }

    /// Create a new transport with proxy auth and configurable CDP timeout.
    pub async fn new_with_options(
        child: Child,
        ws_url: &str,
        proxy_auth: Option<(String, String)>,
        cdp_timeout_secs: u64,
    ) -> Result<Self> {
        let mut child = ChildCleanupGuard::new(child);
        let stream = Self::ws_connect(ws_url).await?;
        Ok(Self::build(
            Some(child.take()?),
            stream,
            proxy_auth,
            cdp_timeout_secs,
            true,
        ))
    }

    /// Connect to an existing Chrome instance at the given WebSocket URL.
    /// Does not manage a Chrome process — caller owns the browser lifecycle.
    pub async fn connect(ws_url: &str, cdp_timeout_secs: u64) -> Result<Self> {
        let stream = Self::ws_connect(ws_url).await?;
        Ok(Self::build(None, stream, None, cdp_timeout_secs, true))
    }

    /// Connect to an existing Chrome with full options control. Use
    /// `filter_cdp = false` to allow `Runtime.enable` and friends — needed
    /// when driving a user-owned browser where stealth filtering is unwanted.
    pub async fn connect_with_options(
        ws_url: &str,
        cdp_timeout_secs: u64,
        filter_cdp: bool,
    ) -> Result<Self> {
        let stream = Self::ws_connect(ws_url).await?;
        Ok(Self::build(
            None,
            stream,
            None,
            cdp_timeout_secs,
            filter_cdp,
        ))
    }

    /// Reader task — reads CDP messages off the WebSocket. tokio-tungstenite
    /// handles framing/fragmentation/close; we only reply to pings and parse
    /// text payloads. If `proxy_auth` is set, `Fetch.authRequired` events are
    /// auto-answered with `Fetch.continueWithAuth`.
    async fn reader_loop(
        mut source: WsSource,
        pending: Arc<PendingMap>,
        event_tx: broadcast::Sender<CdpMessage>,
        writer: SharedSink,
        proxy_auth: Option<(String, String)>,
    ) {
        let exit_reason;
        // Separate command ID space for auth responses (won't collide with main IDs)
        let mut auth_cmd_id: u64 = u64::MAX / 2;

        loop {
            let text = match source.next().await {
                Some(Ok(Message::Text(t))) => t,
                Some(Ok(Message::Binary(b))) => match String::from_utf8(b.to_vec()) {
                    Ok(s) => s.into(),
                    Err(_) => continue,
                },
                Some(Ok(Message::Ping(payload))) => {
                    // Split streams don't auto-pong; reply via the shared sink.
                    let mut w = writer.lock().await;
                    if let Err(error) = w.send(Message::Pong(payload)).await {
                        exit_reason = format!("WebSocket pong write failed: {}", error);
                        tracing::debug!("{}", exit_reason);
                        break;
                    }
                    continue;
                }
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => continue,
                Some(Ok(Message::Close(_))) => {
                    exit_reason = "WebSocket closed by server".to_string();
                    tracing::debug!("{}", exit_reason);
                    break;
                }
                Some(Err(e)) => {
                    exit_reason = format!("WebSocket read error: {}", e);
                    tracing::debug!("{}", exit_reason);
                    break;
                }
                None => {
                    exit_reason = "WebSocket stream ended".to_string();
                    tracing::debug!("{}", exit_reason);
                    break;
                }
            };

            let msg: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Failed to parse CDP message: {} - {}", e, text);
                    continue;
                }
            };

            // Check if response or event
            if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
                let result = if let Some(error) = msg.get("error") {
                    Err(Error::cdp(
                        msg.get("method")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown"),
                        error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1),
                        error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown"),
                    ))
                } else {
                    Ok(msg.get("result").cloned().unwrap_or(json!({})))
                };

                let mut pending_guard = lock_pending(&pending);
                if let Some(sender) = pending_guard.remove(&id) {
                    drop(sender.send(result));
                } else {
                    tracing::trace!("Response for unknown id: {}", id);
                }
            } else if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let session_id = msg
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .map(String::from);

                // Auto-handle proxy auth challenges
                if method == "Fetch.authRequired" {
                    if let Some((ref username, ref password)) = proxy_auth {
                        if let Some(request_id) = params.get("requestId").and_then(|v| v.as_str()) {
                            auth_cmd_id += 1;
                            let mut response = json!({
                                "id": auth_cmd_id,
                                "method": "Fetch.continueWithAuth",
                                "params": {
                                    "requestId": request_id,
                                    "authChallengeResponse": {
                                        "response": "ProvideCredentials",
                                        "username": username,
                                        "password": password
                                    }
                                }
                            });
                            // Include sessionId if present
                            if let Some(ref sid) = session_id {
                                response["sessionId"] = json!(sid);
                            }
                            if let Ok(data) = serde_json::to_string(&response) {
                                let mut w = writer.lock().await;
                                if let Err(e) = w.send(Message::Text(data.into())).await {
                                    tracing::error!(
                                        "Failed to send proxy auth response: {} — connection may be broken",
                                        e
                                    );
                                    exit_reason = format!("Proxy auth write failed: {}", e);
                                    // Break so pending requests fail immediately
                                    // instead of hanging.
                                    break;
                                } else {
                                    tracing::debug!("Auto-responded to proxy auth challenge");
                                }
                            }
                            continue; // Don't forward to event channel
                        }
                    }
                }

                // Publish event; ignore "no active receivers" errors.
                drop(event_tx.send(CdpMessage::Event {
                    method: method.to_string(),
                    params,
                    session_id,
                }));
            }
        }

        // Drain all pending requests so callers fail immediately instead of
        // hanging until the per-command timeout fires.
        let context = format!("WebSocket connection lost: {}", exit_reason);
        let n = fail_pending(&pending, &context);
        if n > 0 {
            tracing::error!(
                "Reader loop exiting ({}), failing {} pending request(s)",
                exit_reason,
                n
            );
        }
        tracing::debug!("CDP reader loop ended ({})", exit_reason);
    }

    /// Internal: send a CDP command with optional session ID
    async fn send_impl<C, R>(&self, session_id: Option<&str>, method: &str, params: &C) -> Result<R>
    where
        C: Serialize,
        R: DeserializeOwned,
    {
        // STEALTH: Block detectable commands - return empty object (deserializes via #[serde(default)])
        if self.filter_cdp && is_blocked(method) {
            tracing::debug!("Blocked CDP command: {}", method);
            return serde_json::from_value(json!({})).map_err(Into::into);
        }

        // STEALTH: Warn on risky commands
        if is_risky(method) {
            tracing::warn!("Risky CDP command (may be detectable): {}", method);
        }

        // Check if the reader task has died (exited)
        if let Some(ref handle) = self.reader_handle {
            if handle.is_finished() {
                return Err(Error::transport(
                    "CDP reader task has exited — WebSocket connection is dead",
                ));
            }
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Build and serialize BEFORE inserting into the pending map.
        // This way, if serialization or write fails, we don't leak a
        // pending oneshot channel that never gets a response.
        let mut msg = json!({
            "id": id,
            "method": method,
            "params": serde_json::to_value(params)?
        });
        if let Some(sid) = session_id {
            msg["sessionId"] = json!(sid);
        }
        let data = serde_json::to_string(&msg)?;

        // Create response channel and register it
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = lock_pending(&self.pending);
            pending.insert(id, tx);
        }

        // Write to WebSocket — clean up pending entry on failure
        if let Err(e) = {
            let mut writer = self.writer.lock().await;
            writer.send(Message::Text(data.into())).await
        } {
            let mut pending = lock_pending(&self.pending);
            pending.remove(&id);
            return Err(Error::transport(format!("WebSocket write failed: {}", e)));
        }

        tracing::trace!("Sent CDP command: {} (id={})", method, id);

        // Wait for response with timeout to prevent deadlock if reader dies
        let result = tokio::time::timeout(self.cmd_timeout, rx)
            .await
            .map_err(|elapsed| {
                // Remove the pending request so the reader doesn't try to send to a dropped channel
                let mut pending = lock_pending(&self.pending);
                pending.remove(&id);
                tracing::warn!(
                    "CDP command '{}' timed out after {}s (id={})",
                    method,
                    self.cmd_timeout.as_secs(),
                    id
                );
                Error::transport(format!(
                    "CDP command '{}' timed out after {}s (id={}): {}",
                    method,
                    self.cmd_timeout.as_secs(),
                    id,
                    elapsed
                ))
            })?
            .map_err(|error| Error::transport(format!("Response channel closed: {}", error)))??;

        let response: R = serde_json::from_value(result)?;
        Ok(response)
    }

    /// Send a CDP command and wait for the response
    pub async fn send<C, R>(&self, method: &str, params: &C) -> Result<R>
    where
        C: Serialize,
        R: DeserializeOwned,
    {
        self.send_impl(None, method, params).await
    }

    /// Send a CDP command to a specific session
    pub async fn send_to_session<C, R>(
        &self,
        session_id: &str,
        method: &str,
        params: &C,
    ) -> Result<R>
    where
        C: Serialize,
        R: DeserializeOwned,
    {
        self.send_impl(Some(session_id), method, params).await
    }

    /// Subscribe to CDP events. Each subscriber gets its own receiver; events
    /// published after subscribing are delivered to all live receivers.
    pub fn subscribe(&self) -> broadcast::Receiver<CdpMessage> {
        self.event_tx.subscribe()
    }

    /// Receive the next event from Chrome (single-consumer back-compat).
    /// Skips over lagged notifications and returns the next available event.
    pub async fn recv_event(&self) -> Option<CdpMessage> {
        let mut rx = self.event_rx.lock().await;
        loop {
            match rx.recv().await {
                Ok(msg) => return Some(msg),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Event receiver lagged, skipped {} event(s)", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Try to receive an event without blocking (single-consumer back-compat).
    pub async fn try_recv_event(&self) -> Option<CdpMessage> {
        let mut rx = self.event_rx.lock().await;
        loop {
            match rx.try_recv() {
                Ok(msg) => return Some(msg),
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    tracing::warn!("Event receiver lagged, skipped {} event(s)", n);
                    continue;
                }
                Err(_) => return None,
            }
        }
    }

    /// Close the transport and kill Chrome
    pub async fn close(&self) -> Result<()> {
        // Send a WebSocket close frame (best-effort).
        {
            let mut writer = self.writer.lock().await;
            if let Err(error) = writer.send(Message::Close(None)).await {
                tracing::debug!("WebSocket close frame was not sent: {}", error);
            }
        }

        if let Some(handle) = &self.reader_handle {
            handle.abort();
        }
        fail_pending(&self.pending, "CDP transport closed");

        if let Some(ref child) = self.child {
            let mut c = child.lock().await;
            stop_child(&mut c)?;
        }
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
        fail_pending(&self.pending, "CDP transport dropped");

        if let Some(ref child) = self.child {
            if let Ok(mut c) = child.try_lock() {
                if let Err(error) = stop_child(&mut c) {
                    tracing::warn!("Failed to stop Chrome while dropping transport: {}", error);
                }
            }
        }
    }
}

/// Launch Chrome and get the WebSocket debugging URL
pub fn launch_chrome(path: &std::path::Path, args: &[String]) -> Result<(Child, String)> {
    use std::process::Command;

    let mut cmd = Command::new(path);
    cmd.args(args)
        .args(["--remote-debugging-port=0"]) // Let Chrome pick a free port
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped()); // We need stderr to get the DevTools URL

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Launch(format!("Failed to launch Chrome: {}", e)))?;

    // Read stderr to find the DevTools URL (with 30s timeout)
    let stderr = child
        .stderr
        .take()
        .ok_or(Error::Launch("No stderr from Chrome".into()))?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        // Chrome prints: DevTools listening on ws://127.0.0.1:PORT/devtools/browser/GUID
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            tracing::trace!("Chrome stderr: {}", line);

            if line.contains("DevTools listening on") {
                if let Some(url_start) = line.find("ws://") {
                    if tx.send(line[url_start..].trim().to_string()).is_err() {
                        tracing::debug!("Chrome launch receiver dropped before URL delivery");
                    }
                    return;
                }
            }
        }
    });

    let ws_url = match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(url) => url,
        Err(channel_error) => {
            // Kill Chrome so a missing DevTools URL can't orphan it.
            if let Err(cleanup_error) = stop_child(&mut child) {
                tracing::warn!(
                    "Failed to clean up Chrome after launch-channel error: {}",
                    cleanup_error
                );
            }
            return Err(Error::Launch(format!(
                "Failed waiting for Chrome DevTools URL: {}",
                channel_error
            )));
        }
    };

    tracing::info!("Chrome DevTools URL: {}", ws_url);

    Ok((child, ws_url))
}
