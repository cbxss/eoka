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

/// A pending request waiting for a response
type PendingRequest = oneshot::Sender<Result<Value>>;

/// WebSocket message types
mod ws {
    pub const OPCODE_CONTINUATION: u8 = 0x0;
    pub const OPCODE_TEXT: u8 = 0x1;
    pub const OPCODE_BINARY: u8 = 0x2;
    pub const OPCODE_CLOSE: u8 = 0x8;
    pub const OPCODE_PING: u8 = 0x9;
    pub const OPCODE_PONG: u8 = 0xA;
}

/// Simple WebSocket frame writer (text opcode)
fn write_ws_frame(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    write_ws_frame_with_opcode(stream, ws::OPCODE_TEXT, data)
}

/// WebSocket frame writer with explicit opcode
fn write_ws_frame_with_opcode(
    stream: &mut TcpStream,
    opcode: u8,
    data: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;

    let len = data.len();
    let mut frame = Vec::with_capacity(14 + len);

    // FIN + opcode
    frame.push(0x80 | opcode);

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
    let mask: [u8; 4] = [
        fastrand::u8(..),
        fastrand::u8(..),
        fastrand::u8(..),
        fastrand::u8(..),
    ];
    frame.extend_from_slice(&mask);

    // Masked payload
    for (i, byte) in data.iter().enumerate() {
        frame.push(byte ^ mask[i % 4]);
    }

    stream.write_all(&frame)?;
    stream.flush()?;
    Ok(())
}

/// Read exactly `buf.len()` bytes of a frame in progress; a mid-frame timeout
/// desyncs the stream, so remap it to a fatal error.
fn read_frame_bytes(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    match stream.read_exact(buf) {
        Ok(()) => Ok(()),
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                format!("mid-frame read timeout (stream desynced): {}", e),
            ))
        }
        Err(e) => Err(e),
    }
}

/// Read a WebSocket frame, returns (fin, opcode, payload). A timeout on the first
/// header byte is a recoverable idle timeout; mid-frame timeouts are fatal.
fn read_ws_frame(stream: &mut TcpStream) -> std::io::Result<(bool, u8, Vec<u8>)> {
    use std::io::Read;

    let mut first = [0u8; 1];
    stream.read_exact(&mut first)?;

    let mut second = [0u8; 1];
    read_frame_bytes(stream, &mut second)?;

    let fin = (first[0] & 0x80) != 0;
    let opcode = first[0] & 0x0F;
    let masked = (second[0] & 0x80) != 0;
    let mut len = (second[0] & 0x7F) as usize;

    if len == 126 {
        let mut ext = [0u8; 2];
        read_frame_bytes(stream, &mut ext)?;
        len = ((ext[0] as usize) << 8) | (ext[1] as usize);
    } else if len == 127 {
        let mut ext = [0u8; 8];
        read_frame_bytes(stream, &mut ext)?;
        const MAX_FRAME_LEN: usize = 256 * 1024 * 1024;
        len = 0;
        for byte in ext.iter() {
            // Use checked_shl to prevent silent overflow on 32-bit platforms
            len = match len.checked_shl(8) {
                Some(shifted) => shifted | (*byte as usize),
                None => MAX_FRAME_LEN + 1, // force the size check below to trigger
            };
            if len > MAX_FRAME_LEN {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("WebSocket frame too large: {} bytes", len),
                ));
            }
        }
    }

    let mask = if masked {
        let mut m = [0u8; 4];
        read_frame_bytes(stream, &mut m)?;
        Some(m)
    } else {
        None
    };

    let mut payload = vec![0u8; len];
    read_frame_bytes(stream, &mut payload)?;

    if let Some(mask) = mask {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    Ok((fin, opcode, payload))
}

/// CDP Transport - handles sending commands and receiving responses via WebSocket
pub struct Transport {
    /// The Chrome child process (None when connecting to an existing instance)
    child: Option<Mutex<Child>>,
    /// WebSocket stream for writing (shared with proxy auth via Arc)
    writer: Arc<std::sync::Mutex<TcpStream>>,
    /// Next message ID
    next_id: AtomicU64,
    /// Pending requests waiting for responses
    pending: Arc<PendingMap>,
    /// Channel to receive parsed messages from the reader task
    event_rx: Mutex<mpsc::Receiver<CdpMessage>>,
    /// Timeout for CDP commands
    cmd_timeout: std::time::Duration,
    /// Reader thread handle — checked for panics before sending commands
    reader_handle: Option<std::thread::JoinHandle<()>>,
    /// When true, drop "detectable" commands (`Runtime.enable`, etc.) silently.
    /// Defaults to true. Disable when you own the browser session and need
    /// full DevTools-equivalent control.
    filter_cdp: bool,
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

    /// Perform WebSocket handshake and return the connected stream.
    fn ws_handshake(ws_url: &str) -> Result<TcpStream> {
        if !ws_url.starts_with("ws://") {
            return Err(Error::transport(format!(
                "Invalid WebSocket URL (expected ws://...): {}",
                ws_url
            )));
        }
        let url = ws_url.trim_start_matches("ws://");
        let (host_port, _path) = url.split_once('/').unwrap_or((url, ""));

        let mut stream = TcpStream::connect(host_port)
            .map_err(|e| Error::transport_io("Failed to connect to Chrome", e))?;

        let path = format!("/{}", url.split_once('/').map(|(_, p)| p).unwrap_or(""));
        let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, {
            let mut buf = [0u8; 16];
            for b in buf.iter_mut() {
                *b = fastrand::u8(..);
            }
            buf
        });

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

        // Read handshake response until end of HTTP headers.
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

        if !response_str.starts_with("HTTP/1.1 101") {
            return Err(Error::transport(format!(
                "WebSocket handshake failed: {}",
                response_str
            )));
        }

        tracing::debug!("WebSocket connected to {}", ws_url);
        Ok(stream)
    }

    /// Spawn the reader thread and build the Transport from a handshaked stream.
    fn build(
        child: Option<Child>,
        stream: TcpStream,
        proxy_auth: Option<(String, String)>,
        cdp_timeout_secs: u64,
        filter_cdp: bool,
    ) -> Result<Self> {
        let cmd_timeout = std::time::Duration::from_secs(cdp_timeout_secs);
        tracing::debug!("CDP timeout set to {}s", cdp_timeout_secs);

        // Clone stream for reader — set TCP read timeout as safety net
        let reader_stream = stream
            .try_clone()
            .map_err(|e| Error::transport_io("Failed to clone stream", e))?;
        reader_stream
            .set_read_timeout(Some(std::time::Duration::from_secs(cdp_timeout_secs * 4)))
            .map_err(|e| Error::transport_io("Failed to set read timeout", e))?;

        let writer = Arc::new(std::sync::Mutex::new(stream));

        // If proxy auth is configured, share the writer so auth responses
        // don't race with command writes on the TCP stream.
        let proxy_auth_writer =
            proxy_auth.map(|(username, password)| (username, password, Arc::clone(&writer)));

        let pending: Arc<PendingMap> = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::channel(256);

        let pending_clone = Arc::clone(&pending);
        let reader_handle = std::thread::spawn(move || {
            Self::reader_loop(reader_stream, pending_clone, event_tx, proxy_auth_writer);
        });

        Ok(Self {
            child: child.map(Mutex::new),
            writer,
            next_id: AtomicU64::new(1),
            pending,
            event_rx: Mutex::new(event_rx),
            cmd_timeout,
            reader_handle: Some(reader_handle),
            filter_cdp,
        })
    }

    /// Create a new transport with proxy auth and configurable CDP timeout.
    pub fn new_with_options(
        mut child: Child,
        ws_url: &str,
        proxy_auth: Option<(String, String)>,
        cdp_timeout_secs: u64,
    ) -> Result<Self> {
        // Kill the child if the handshake fails, so it can't orphan Chrome.
        let stream = match Self::ws_handshake(ws_url) {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        Self::build(Some(child), stream, proxy_auth, cdp_timeout_secs, true)
    }

    /// Connect to an existing Chrome instance at the given WebSocket URL.
    /// Does not manage a Chrome process — caller owns the browser lifecycle.
    pub fn connect(ws_url: &str, cdp_timeout_secs: u64) -> Result<Self> {
        let stream = Self::ws_handshake(ws_url)?;
        Self::build(None, stream, None, cdp_timeout_secs, true)
    }

    /// Connect to an existing Chrome with full options control. Use
    /// `filter_cdp = false` to allow `Runtime.enable` and friends — needed
    /// when driving a user-owned browser where stealth filtering is unwanted.
    pub fn connect_with_options(
        ws_url: &str,
        cdp_timeout_secs: u64,
        filter_cdp: bool,
    ) -> Result<Self> {
        let stream = Self::ws_handshake(ws_url)?;
        Self::build(None, stream, None, cdp_timeout_secs, filter_cdp)
    }

    /// Reader loop - runs in a separate thread to read from WebSocket.
    /// If `proxy_auth` is provided, `Fetch.authRequired` events are automatically
    /// answered with `Fetch.continueWithAuth` using the given credentials.
    fn reader_loop(
        mut stream: TcpStream,
        pending: Arc<PendingMap>,
        event_tx: mpsc::Sender<CdpMessage>,
        proxy_auth: Option<(String, String, Arc<std::sync::Mutex<TcpStream>>)>,
    ) {
        let exit_reason;
        // Separate command ID space for auth responses (won't collide with main IDs)
        let mut auth_cmd_id: u64 = u64::MAX / 2;
        // Track consecutive timeouts to detect a dead connection
        let mut consecutive_timeouts: u32 = 0;
        const MAX_CONSECUTIVE_TIMEOUTS: u32 = 3;

        // Reassembly state for fragmented data messages (RFC 6455 §5.4).
        const MAX_MESSAGE_LEN: usize = 512 * 1024 * 1024;
        let mut frag_opcode: Option<u8> = None;
        let mut frag_buf: Vec<u8> = Vec::new();

        loop {
            let (fin, opcode, payload) = match read_ws_frame(&mut stream) {
                Ok(frame) => {
                    consecutive_timeouts = 0; // Reset on successful read
                    frame
                }
                Err(e) => {
                    // Read timeout fired — connection may still be alive.
                    // Check if there are any pending requests; if so, this is
                    // suspicious (Chrome should have responded by now).
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut
                    {
                        consecutive_timeouts += 1;
                        let n_pending = pending.lock().unwrap().len();
                        if n_pending > 0 {
                            tracing::warn!(
                                "WebSocket read timeout ({}/{}) with {} pending request(s)",
                                consecutive_timeouts,
                                MAX_CONSECUTIVE_TIMEOUTS,
                                n_pending
                            );
                        }
                        if consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
                            exit_reason = format!(
                                "WebSocket connection dead: {} consecutive read timeouts",
                                consecutive_timeouts
                            );
                            tracing::error!("{}", exit_reason);
                            break;
                        }
                        continue;
                    }
                    exit_reason = format!("WebSocket read error: {}", e);
                    tracing::debug!("{}", exit_reason);
                    break;
                }
            };

            // Reassemble fragmented data messages; control frames are handled inline.
            let message: Vec<u8> = match opcode {
                ws::OPCODE_PING => {
                    // RFC 6455 §5.5.3: Pong must echo the ping's payload
                    if let Err(e) =
                        write_ws_frame_with_opcode(&mut stream, ws::OPCODE_PONG, &payload)
                    {
                        exit_reason = format!("Pong write failed: {}", e);
                        break;
                    }
                    continue;
                }
                ws::OPCODE_PONG => continue,
                ws::OPCODE_CLOSE => {
                    exit_reason = "WebSocket closed by server".to_string();
                    tracing::debug!("{}", exit_reason);
                    break;
                }
                ws::OPCODE_TEXT | ws::OPCODE_BINARY => {
                    if frag_opcode.is_some() {
                        tracing::warn!(
                            "New data frame started mid-fragment; discarding partial message"
                        );
                        frag_buf.clear();
                        frag_opcode = None;
                    }
                    if fin {
                        payload
                    } else {
                        frag_opcode = Some(opcode);
                        frag_buf = payload;
                        continue;
                    }
                }
                ws::OPCODE_CONTINUATION => {
                    if frag_opcode.is_none() {
                        tracing::warn!("Continuation frame with no message in progress; ignoring");
                        continue;
                    }
                    if frag_buf.len() + payload.len() > MAX_MESSAGE_LEN {
                        exit_reason = format!(
                            "Reassembled WebSocket message exceeds {} byte cap",
                            MAX_MESSAGE_LEN
                        );
                        tracing::error!("{}", exit_reason);
                        break;
                    }
                    frag_buf.extend_from_slice(&payload);
                    if fin {
                        frag_opcode = None;
                        std::mem::take(&mut frag_buf)
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            {
                let text = match String::from_utf8(message) {
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
                        if let Some((ref username, ref password, ref writer)) = proxy_auth {
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
                                    let mut w = writer.lock().unwrap();
                                    if let Err(e) = write_ws_frame(&mut w, data.as_bytes()) {
                                        tracing::error!(
                                                "Failed to send proxy auth response: {} — connection may be broken",
                                                e
                                            );
                                        exit_reason = format!("Proxy auth write failed: {}", e);
                                        // Break out of the loop so pending requests
                                        // get failed immediately instead of hanging.
                                        break;
                                    } else {
                                        tracing::debug!("Auto-responded to proxy auth challenge");
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
        }

        // Drain all pending requests so callers fail immediately instead of
        // hanging until the per-command timeout fires.
        let mut pending_guard = pending.lock().unwrap();
        let n = pending_guard.len();
        if n > 0 {
            tracing::error!(
                "Reader loop exiting ({}), failing {} pending request(s)",
                exit_reason,
                n
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
        if self.filter_cdp && is_blocked(method) {
            tracing::debug!("Blocked CDP command: {}", method);
            return serde_json::from_value(json!({})).map_err(Into::into);
        }

        // STEALTH: Warn on risky commands
        if is_risky(method) {
            tracing::warn!("Risky CDP command (may be detectable): {}", method);
        }

        // Check if the reader thread has died (panicked or exited)
        if let Some(ref handle) = self.reader_handle {
            if handle.is_finished() {
                return Err(Error::transport(
                    "CDP reader thread has exited — WebSocket connection is dead",
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
            let mut pending = self.pending.lock().unwrap();
            pending.insert(id, tx);
        }

        // Write to WebSocket — clean up pending entry on failure
        if let Err(e) = {
            let mut writer = self.writer.lock().unwrap();
            write_ws_frame(&mut writer, data.as_bytes())
        } {
            let mut pending = self.pending.lock().unwrap();
            pending.remove(&id);
            return Err(Error::transport_io("WebSocket write failed", e));
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
                    method,
                    self.cmd_timeout.as_secs(),
                    id
                );
                Error::transport(format!(
                    "CDP command '{}' timed out after {}s (id={})",
                    method,
                    self.cmd_timeout.as_secs(),
                    id
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
            let mut writer = self.writer.lock().unwrap();
            let close_frame = vec![0x80 | ws::OPCODE_CLOSE, 0x80, 0, 0, 0, 0];
            let _ = std::io::Write::write_all(&mut *writer, &close_frame);
        }

        if let Some(ref child) = self.child {
            let mut c = child.lock().await;
            let _ = c.kill();
            let _ = c.wait();
        }
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        if let Some(ref child) = self.child {
            if let Ok(mut c) = child.try_lock() {
                let _ = c.kill();
                // Reap to avoid a zombie.
                let _ = c.wait();
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
                    let _ = tx.send(line[url_start..].trim().to_string());
                    return;
                }
            }
        }
    });

    let ws_url = match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(url) => url,
        Err(_) => {
            // Kill Chrome so a missing DevTools URL can't orphan it.
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Launch(
                "Timed out waiting for Chrome DevTools URL (30s)".into(),
            ));
        }
    };

    tracing::info!("Chrome DevTools URL: {}", ws_url);

    Ok((child, ws_url))
}
