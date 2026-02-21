//! CDP Transport Layer
//!
//! Handles communication with Chrome via WebSocket.
//! Includes built-in filtering to block detectable CDP commands.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};

/// Pending requests map - uses std::sync::Mutex because it's accessed from both
/// a std thread (reader loop) and async contexts, and the lock is held very briefly.
type PendingMap = std::sync::Mutex<HashMap<u64, PendingRequest>>;

use crate::error::{Error, Result};

/// Commands that are blocked (highly detectable by anti-bot)
const BLOCKED_COMMANDS: &[&str] = &[
    "Runtime.enable",
    "Runtime.disable",
    "HeapProfiler.enable",
    "HeapProfiler.disable",
    "Profiler.enable",
    "Profiler.disable",
    "Debugger.enable",
    "Debugger.disable",
    "Console.enable",
    "Console.disable",
];

/// Commands that trigger a warning (potentially detectable)
const RISKY_COMMANDS: &[&str] = &[
    "Emulation.setUserAgentOverride",
    "Emulation.setTimezoneOverride",
    "Emulation.setDeviceMetricsOverride",
    "Page.setBypassCSP",
];

/// Check if a command should be blocked
fn is_blocked(method: &str) -> bool {
    BLOCKED_COMMANDS.contains(&method)
}

/// Check if a command is risky
fn is_risky(method: &str) -> bool {
    RISKY_COMMANDS.contains(&method)
}

/// A pending request waiting for a response
type PendingRequest = oneshot::Sender<Result<Value>>;

/// WebSocket message types
mod ws {
    pub const OPCODE_TEXT: u8 = 0x1;
    pub const OPCODE_CLOSE: u8 = 0x8;
    pub const OPCODE_PING: u8 = 0x9;
    pub const OPCODE_PONG: u8 = 0xA;
}

/// Simple WebSocket frame writer
fn write_ws_frame(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let len = data.len();
    let mut frame = Vec::with_capacity(14 + len);

    // FIN + text opcode
    frame.push(0x80 | ws::OPCODE_TEXT);

    // Mask bit set (client must mask), then length
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len < 65536 {
        frame.push(0x80 | 126);
        frame.push((len >> 8) as u8);
        frame.push(len as u8);
    } else {
        frame.push(0x80 | 127);
        for i in (0..8).rev() {
            frame.push((len >> (i * 8)) as u8);
        }
    }

    // Random masking key per frame (RFC 6455 compliance)
    let mask: [u8; 4] = rand::random();
    frame.extend_from_slice(&mask);

    // Masked payload
    for (i, byte) in data.iter().enumerate() {
        frame.push(byte ^ mask[i % 4]);
    }

    stream.write_all(&frame)?;
    stream.flush()?;
    Ok(())
}

/// Read a WebSocket frame, returns (opcode, payload)
fn read_ws_frame(stream: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    use std::io::Read;

    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;

    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut len = (header[1] & 0x7F) as usize;

    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        len = ((ext[0] as usize) << 8) | (ext[1] as usize);
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        len = 0;
        for byte in ext.iter() {
            len = (len << 8) | (*byte as usize);
        }
    }

    let mask = if masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };

    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;

    if let Some(mask) = mask {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    Ok((opcode, payload))
}

/// CDP Transport - handles sending commands and receiving responses via WebSocket
pub struct Transport {
    /// The Chrome child process
    child: Mutex<Child>,
    /// WebSocket stream for writing
    writer: Mutex<TcpStream>,
    /// Next message ID
    next_id: AtomicU64,
    /// Pending requests waiting for responses
    pending: Arc<PendingMap>,
    /// Channel to receive parsed messages from the reader task
    event_rx: Mutex<mpsc::Receiver<CdpMessage>>,
    /// Timeout for CDP commands
    cmd_timeout: std::time::Duration,
}

/// A parsed CDP message (response or event)
#[derive(Debug)]
pub enum CdpMessage {
    Response {
        id: u64,
        result: Result<Value>,
    },
    Event {
        method: String,
        params: Value,
        session_id: Option<String>,
    },
}

impl Transport {
    /// Create a new transport connecting to Chrome via WebSocket
    pub fn new(child: Child, ws_url: &str) -> Result<Self> {
        Self::new_with_options(child, ws_url, None, 30)
    }

    /// Create a new transport with optional proxy authentication credentials.
    /// When provided, the reader loop will automatically respond to
    /// `Fetch.authRequired` events with the given username/password.
    pub fn new_with_proxy_auth(
        child: Child,
        ws_url: &str,
        proxy_auth: Option<(String, String)>,
    ) -> Result<Self> {
        Self::new_with_options(child, ws_url, proxy_auth, 30)
    }

    /// Create a new transport with proxy auth and configurable CDP timeout.
    pub fn new_with_options(
        child: Child,
        ws_url: &str,
        proxy_auth: Option<(String, String)>,
        cdp_timeout_secs: u64,
    ) -> Result<Self> {
        // Parse WebSocket URL
        let url = ws_url.trim_start_matches("ws://");
        let (host_port, _path) = url.split_once('/').unwrap_or((url, ""));

        // Connect TCP
        let mut stream = TcpStream::connect(host_port)
            .map_err(|e| Error::transport_io("Failed to connect to Chrome", e))?;

        // WebSocket handshake
        let path = format!("/{}", url.split_once('/').map(|(_, p)| p).unwrap_or(""));
        let key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            rand::random::<[u8; 16]>(),
        );

        let handshake = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            path, host_port, key
        );

        use std::io::{BufRead, Write};
        stream
            .write_all(handshake.as_bytes())
            .map_err(|e| Error::transport_io("Handshake write failed", e))?;

        // Read handshake response - loop until we see the end of HTTP headers.
        // Use BufReader to avoid a syscall per byte.
        let mut reader = std::io::BufReader::new(&stream);
        let mut response_buf = Vec::with_capacity(1024);
        loop {
            let available = reader
                .fill_buf()
                .map_err(|e| Error::transport_io("Handshake read failed", e))?;
            if available.is_empty() {
                return Err(Error::transport(
                    "Connection closed during WebSocket handshake",
                ));
            }
            let len = available.len();
            response_buf.extend_from_slice(available);
            reader.consume(len);
            if response_buf.len() >= 4 && response_buf[response_buf.len() - 4..] == *b"\r\n\r\n" {
                break;
            }
            if response_buf.len() > 8192 {
                return Err(Error::transport("WebSocket handshake response too large"));
            }
        }
        let response_str = String::from_utf8_lossy(&response_buf);

        if !response_str.contains("101") {
            return Err(Error::transport(format!(
                "WebSocket handshake failed: {}",
                response_str
            )));
        }

        tracing::debug!("WebSocket connected to {}", ws_url);

        let cmd_timeout = std::time::Duration::from_secs(cdp_timeout_secs);
        tracing::debug!("CDP timeout set to {}s", cdp_timeout_secs);

        // Clone stream for reader — set TCP read timeout as safety net
        // so the reader thread doesn't block forever if Chrome hangs
        let reader_stream = stream
            .try_clone()
            .map_err(|e| Error::transport_io("Failed to clone stream", e))?;
        reader_stream
            .set_read_timeout(Some(std::time::Duration::from_secs(cdp_timeout_secs * 4)))
            .map_err(|e| Error::transport_io("Failed to set read timeout", e))?;

        // If proxy auth is configured, clone writer for the reader loop to send
        // Fetch.continueWithAuth responses directly
        let proxy_auth_writer = match proxy_auth {
            Some((username, password)) => {
                let auth_writer = stream
                    .try_clone()
                    .map_err(|e| Error::transport_io("Failed to clone auth writer", e))?;
                Some((username, password, auth_writer))
            }
            None => None,
        };

        let pending: Arc<PendingMap> = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::channel(256);

        // Spawn reader task
        let pending_clone = Arc::clone(&pending);
        std::thread::spawn(move || {
            Self::reader_loop(reader_stream, pending_clone, event_tx, proxy_auth_writer);
        });

        Ok(Self {
            child: Mutex::new(child),
            writer: Mutex::new(stream),
            next_id: AtomicU64::new(1),
            pending,
            event_rx: Mutex::new(event_rx),
            cmd_timeout,
        })
    }

    /// Reader loop - runs in a separate thread to read from WebSocket.
    /// If `proxy_auth` is provided, `Fetch.authRequired` events are automatically
    /// answered with `Fetch.continueWithAuth` using the given credentials.
    fn reader_loop(
        mut stream: TcpStream,
        pending: Arc<PendingMap>,
        event_tx: mpsc::Sender<CdpMessage>,
        mut proxy_auth: Option<(String, String, TcpStream)>,
    ) {
        let exit_reason;
        // Separate command ID space for auth responses (won't collide with main IDs)
        let mut auth_cmd_id: u64 = u64::MAX / 2;

        loop {
            let (opcode, payload) = match read_ws_frame(&mut stream) {
                Ok(frame) => frame,
                Err(e) => {
                    // Read timeout fired — connection may still be alive.
                    // Check if there are any pending requests; if so, this is
                    // suspicious (Chrome should have responded by now).
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut
                    {
                        let n_pending = pending.lock().unwrap().len();
                        if n_pending > 0 {
                            tracing::warn!(
                                "WebSocket read timeout with {} pending request(s)",
                                n_pending
                            );
                        }
                        continue;
                    }
                    exit_reason = format!("WebSocket read error: {}", e);
                    tracing::debug!("{}", exit_reason);
                    break;
                }
            };

            match opcode {
                ws::OPCODE_TEXT => {
                    let text = match String::from_utf8(payload) {
                        Ok(s) => s,
                        Err(_) => continue,
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

                        let mut pending_guard = pending.lock().unwrap();
                        if let Some(sender) = pending_guard.remove(&id) {
                            let _ = sender.send(result);
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
                            if let Some((ref username, ref password, ref mut writer)) = proxy_auth {
                                if let Some(request_id) =
                                    params.get("requestId").and_then(|v| v.as_str())
                                {
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
                                        if let Err(e) =
                                            write_ws_frame(writer, data.as_bytes())
                                        {
                                            tracing::warn!(
                                                "Failed to send proxy auth response: {}",
                                                e
                                            );
                                        } else {
                                            tracing::debug!(
                                                "Auto-responded to proxy auth challenge"
                                            );
                                        }
                                    }
                                    continue; // Don't forward to event channel
                                }
                            }
                        }

                        if event_tx
                            .try_send(CdpMessage::Event {
                                method: method.to_string(),
                                params,
                                session_id,
                            })
                            .is_err()
                        {
                            // Channel full or closed — drop event to avoid blocking reader
                            tracing::trace!("Event channel full, dropping: {}", method);
                        }
                    }
                }
                ws::OPCODE_PING => {
                    // Respond with pong
                    let frame = vec![0x80 | ws::OPCODE_PONG, 0x80, 0, 0, 0, 0];
                    let _ = std::io::Write::write_all(&mut stream, &frame);
                }
                ws::OPCODE_CLOSE => {
                    exit_reason = "WebSocket closed by server".to_string();
                    tracing::debug!("{}", exit_reason);
                    break;
                }
                _ => {}
            }
        }

        // Drain all pending requests so callers fail immediately instead of
        // hanging until the per-command timeout fires.
        let mut pending_guard = pending.lock().unwrap();
        let n = pending_guard.len();
        if n > 0 {
            eprintln!(
                "[eoka] reader loop exiting ({}), failing {} pending request(s)",
                exit_reason, n
            );
            for (_id, sender) in pending_guard.drain() {
                let _ = sender.send(Err(Error::transport(format!(
                    "WebSocket connection lost: {}",
                    exit_reason
                ))));
            }
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
        if is_blocked(method) {
            tracing::debug!("Blocked CDP command: {}", method);
            return serde_json::from_value(json!({})).map_err(Into::into);
        }

        // STEALTH: Warn on risky commands
        if is_risky(method) {
            tracing::warn!("Risky CDP command (may be detectable): {}", method);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Create response channel
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            pending.insert(id, tx);
        }

        // Build message
        let mut msg = json!({
            "id": id,
            "method": method,
            "params": serde_json::to_value(params)?
        });
        if let Some(sid) = session_id {
            msg["sessionId"] = json!(sid);
        }

        let data = serde_json::to_string(&msg)?;

        {
            let mut writer = self.writer.lock().await;
            write_ws_frame(&mut writer, data.as_bytes())
                .map_err(|e| Error::transport_io("WebSocket write failed", e))?;
        }

        tracing::trace!("Sent CDP command: {} (id={})", method, id);

        // Wait for response with timeout to prevent deadlock if reader dies
        let result = tokio::time::timeout(self.cmd_timeout, rx)
            .await
            .map_err(|_| {
                // Remove the pending request so the reader doesn't try to send to a dropped channel
                let mut pending = self.pending.lock().unwrap();
                pending.remove(&id);
                tracing::warn!(
                    "CDP command '{}' timed out after {}s (id={})",
                    method, self.cmd_timeout.as_secs(), id
                );
                Error::transport(format!(
                    "CDP command '{}' timed out after {}s (id={})",
                    method, self.cmd_timeout.as_secs(), id
                ))
            })?
            .map_err(|_| Error::transport("Response channel closed"))??;

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

    /// Receive the next event from Chrome
    pub async fn recv_event(&self) -> Option<CdpMessage> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    /// Try to receive an event without blocking
    pub async fn try_recv_event(&self) -> Option<CdpMessage> {
        let mut rx = self.event_rx.lock().await;
        rx.try_recv().ok()
    }

    /// Close the transport and kill Chrome
    pub async fn close(&self) -> Result<()> {
        // Send WebSocket close frame
        {
            let mut writer = self.writer.lock().await;
            let close_frame = vec![0x80 | ws::OPCODE_CLOSE, 0x80, 0, 0, 0, 0];
            let _ = std::io::Write::write_all(&mut *writer, &close_frame);
        }

        let mut child = self.child.lock().await;
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        // Try to kill Chrome process on drop
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.kill();
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
                    let _ = tx.send(line[url_start..].trim().to_string());
                    return;
                }
            }
        }
    });

    let ws_url = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .map_err(|_| Error::Launch("Timed out waiting for Chrome DevTools URL (30s)".into()))?;

    tracing::info!("Chrome DevTools URL: {}", ws_url);

    Ok((child, ws_url))
}
