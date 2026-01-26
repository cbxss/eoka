# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Build & Test

```bash
cargo build                          # Build library
cargo build --examples               # Build examples
cargo run --example basic            # Basic usage
cargo run --example detection_test   # Bot detection tests (sannysoft, browserleaks, etc.)
cargo run --example rebrowser_test   # Rebrowser bot detector test
cargo run --example detection_test -- --visible  # Visible browser
cargo run --example request_capture  # HTTP request capture demo
```

## Architecture

```
src/
├── lib.rs              # Public API: Browser, Page, StealthConfig, Result
├── browser.rs          # Chrome launcher, stealth args
├── page.rs             # Page abstraction, Element, request capture
├── session.rs          # Cookie import/export
├── error.rs            # Error types
├── cdp/
│   ├── transport.rs    # WebSocket client + command filtering
│   ├── connection.rs   # Browser/Session CDP wrappers
│   └── types.rs        # Hand-written CDP types (~30 commands)
└── stealth/
    ├── evasions.rs     # 15 JavaScript injection scripts
    ├── patcher.rs      # Binary patching (Aho-Corasick)
    ├── human.rs        # Bezier curves, typing simulation
    └── fingerprint.rs  # User agent generation
```

## Key Design Decisions

### CDP Command Filtering
Transport blocks detectable commands at `src/cdp/transport.rs:20-30`:
- `Runtime.enable` - BLOCKED (prevents consoleAPICalled detection)
- `Debugger.enable` - BLOCKED
- `HeapProfiler.*` - BLOCKED
- `Console.enable` - BLOCKED

### Document Proxy
CDP markers ($cdc_*) are hidden via Proxy on document object. See `src/stealth/evasions.rs` CDP_EVASION.

### Navigator Prototype
All navigator properties (webdriver, plugins, getBattery) are defined on `Navigator.prototype`, not the instance. This prevents detection via `Object.getOwnPropertyNames(navigator)`.

### WebSocket Transport
Uses std::net::TcpStream with blocking reader thread. WebSocket framing is hand-written (no external deps).

### Binary Patching
Uses Aho-Corasick for O(n) multi-pattern matching. Creates symlinked bundle on macOS to avoid copying entire .app.

## Evasion Scripts

Located in `src/stealth/evasions.rs`:

| Script | Purpose |
|--------|---------|
| `WEBDRIVER_EVASION` | navigator.webdriver = false |
| `CDP_EVASION` | Proxy on document to hide $cdc_* markers |
| `CHROME_RUNTIME_EVASION` | chrome.runtime/loadTimes/csi APIs |
| `PERMISSIONS_EVASION` | Fix Notification/Permissions consistency |
| `PLUGINS_EVASION` | Spoof navigator.plugins (3 plugins) |
| `NAVIGATOR_PROPS_EVASION` | languages, platform, hardware |
| `HEADLESS_EVASION` | Screen dimensions, Image fix |
| `BATTERY_EVASION` | navigator.getBattery() |
| `NAVIGATOR_EXTRA_EVASION` | userAgentData, connection |
| `FINGERPRINT_EVASION` | WebGL/Canvas/Audio noise |
| `WEBRTC_EVASION` | Prevent IP leak via STUN |
| `SPEECH_EVASION` | speechSynthesis.getVoices() |
| `MEDIA_DEVICES_EVASION` | mediaDevices.enumerateDevices() |
| `BLUETOOTH_EVASION` | navigator.bluetooth API |
| `TIMEZONE_EVASION` | Intl.DateTimeFormat consistency |

## Common Tasks

### Add new CDP command
1. Add types to `src/cdp/types.rs`
2. Add method to `Session` in `src/cdp/connection.rs`
3. Check if command should be blocked/warned in `transport.rs`

### Add new evasion
1. Add const to `src/stealth/evasions.rs`
2. Add to `build_evasion_script()` function
3. Test with `cargo run --example rebrowser_test`

### Test detection bypass
```bash
cargo run --example detection_test    # sannysoft, browserleaks, creepjs
cargo run --example rebrowser_test    # Runtime.enable leak, etc.
# Check screenshots: sannysoft.png, rebrowser.png, etc.
```

## Dependencies

Minimal by design:
- `tokio` - async runtime
- `serde`/`serde_json` - serialization
- `aho-corasick` - binary patching
- `memmap2` - memory-mapped file I/O
- `rand` - human simulation randomness
- `base64` - screenshot/response encoding
- `thiserror` - error types
- `tracing` - logging
