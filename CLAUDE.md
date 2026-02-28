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
├── error.rs            # Error types (ElementNotVisible, RetryExhausted, etc.)
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

## Public API Overview

### Browser
- `Browser::launch()` / `Browser::launch_with_config(config)`
- `browser.new_page(url)` - Create page and navigate
- `browser.tabs()` - List all open tabs (returns `Vec<TabInfo>`)
- `browser.activate_tab(id)` - Focus a tab
- `browser.close_tab(id)` - Close a specific tab
- `browser.close()`

### Page - Finding Elements
- `page.find(selector)` / `page.find_all(selector)` - By CSS selector
- `page.find_by_text(text)` - By visible text (prioritizes links/buttons)
- `page.find_all_by_text(text)` - All elements with text
- `page.find_any(&[selectors])` - First matching selector
- `page.exists(selector)` / `page.text_exists(text)` - Check existence

### Page - Clicking
- `page.click(selector)` / `page.human_click(selector)` - Standard click
- `page.click_by_text(text)` / `page.human_click_by_text(text)` - By text
- `page.try_click(selector)` - Returns `Ok(false)` if not found/visible
- `page.try_click_by_text(text)` / `page.try_human_click(selector)`

### Page - Form Filling
- `page.fill(selector, value)` - Clear and type
- `page.human_fill(selector, value)` - Human-like clear and type
- `page.type_into(selector, text)` - Type without clearing
- `page.human_type(selector, text)` - Human-like typing

### Page - Waiting
- `page.wait_for(selector, timeout)` - Wait for element in DOM
- `page.wait_for_visible(selector, timeout)` - Wait for element to be clickable
- `page.wait_for_hidden(selector, timeout)` - Wait for element to disappear
- `page.wait_for_any(&[selectors], timeout)` - Wait for any selector
- `page.wait_for_text(text, timeout)` - Wait for text to appear
- `page.wait_for_url_contains(pattern, timeout)` - Wait for URL pattern
- `page.wait_for_url_change(timeout)` - Wait for navigation
- `page.wait_for_network_idle(idle_ms, timeout)` - Wait for XHR/fetch to complete
- `page.wait(ms)` - Fixed delay

### Page - Info & Debug
- `page.url()` / `page.title()` / `page.content()` / `page.text()`
- `page.target_id()` - Get tab identifier (for multi-tab)
- `page.screenshot()` / `page.screenshot_jpeg(quality)`
- `page.debug_state()` - Returns `PageState` with element counts
- `page.debug_screenshot(prefix)` - Timestamped screenshot

### Page - JavaScript & Frames
- `page.evaluate(js)` / `page.execute(js)` - Run JavaScript
- `page.frames()` - List all frames
- `page.evaluate_in_frame(frame_selector, js)` - JS in iframe (uses Function constructor, CSP-safe)

### Page - File Uploads
- `page.upload_file(selector, path)` - Upload single file
- `page.upload_files(selector, &[paths])` - Upload multiple files

### Page - Select/Dropdowns
- `page.select(selector, value)` - Select by value
- `page.select_by_text(selector, text)` - Select by visible text
- `page.select_multiple(selector, &[values])` - Multi-select

### Page - Hover
- `page.hover(selector)` - Move mouse to element (reveal menus)
- `page.human_hover(selector)` - Human-like hover

### Page - Keyboard
- `page.press_key(key)` - Press key with modifiers (`Enter`, `Ctrl+A`, `Cmd+C`)
- `page.select_all()` / `page.copy()` / `page.paste()` - Platform-aware clipboard

### Page - Utilities
- `page.with_retry(attempts, delay_ms, operation)` - Retry flaky operations
- `page.cookies()` / `page.set_cookie()` / `page.delete_cookie()`

### Element
- `elem.click()` / `elem.human_click()` - Click
- `elem.type_text(text)` / `elem.focus()` - Input
- `elem.is_visible()` - Check if rendered (returns `Result<bool>`)
- `elem.bounding_box()` - Get position/size (handles rotated elements)
- `elem.get_attribute(name)` - Get attribute
- `elem.tag_name()` / `elem.value()` / `elem.text()`
- `elem.is_enabled()` / `elem.is_checked()` - State
- `elem.css(property)` - Computed style
- `elem.scroll_into_view()` - Scroll into viewport

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

### Text Finding Priority
`find_by_text()` searches in two passes:
1. Interactive elements: `a, button, input[type="submit"], [role="button"], [onclick]`
2. Static elements: `div, span, p, label, h1-h6, li, td, th`

### Error Handling
CDP "box model" errors are converted to friendly `ElementNotVisible` errors.
Try-click methods return `Ok(false)` for both missing AND invisible elements.

## Error Types

```rust
Error::ElementNotFound(selector)      // Not in DOM
Error::ElementNotVisible { selector } // In DOM but not rendered
Error::Timeout(message)
Error::RetryExhausted { attempts, last_error }
Error::Cdp { method, code, message }  // Raw CDP error
```

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

### Add new Page method
1. Add method to `impl Page` in `src/page.rs`
2. For text-based methods, use marker attribute pattern (data-eoka-text-match)
3. Update README.md API reference
4. Update this file

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
- `fastrand` - human simulation randomness
- `base64` - screenshot/response encoding
- `thiserror` - error types
- `tracing` - logging

## Exported Types

```rust
pub use browser::{Browser, TabInfo};
pub use error::{Error, Result};
pub use page::{
    BoundingBox,      // Element position/size
    CapturedRequest,  // Network request info
    Element,          // DOM element wrapper
    FrameInfo,        // Frame/iframe info
    Page,             // Page abstraction
    PageState,        // Debug info (url, title, element counts)
    ResponseBody,     // Text or Binary response
    TextMatch,        // Exact, Contains, StartsWith, EndsWith
};
pub use stealth::HumanSpeed;
pub struct StealthConfig { ... }
```
