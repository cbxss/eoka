# Changelog

All notable changes to this project will be documented in this file.

## [0.4.0] - 2026-07-06

### Fixed

#### Transport
- Reassemble fragmented WebSocket frames (previously dropped, hanging the request)
- A mid-frame read timeout is now fatal instead of silently desyncing the stream
- Kill and reap Chrome on failed launch/handshake and DevTools-URL timeout so a partial launch can't orphan a process
- `Error` Display now surfaces the underlying io cause

#### Stealth
- A single `Fingerprint` now drives the UA, `Sec-CH-UA*` client-hint metadata, `navigator.platform`, WebGL and the injected script — previously hardcoded values contradicted the UA (e.g. `MacIntel` on a Windows UA)
- Added `Function.prototype.toString` native masking; stable `navigator.plugins` reference; deterministic canvas/audio noise
- Removed the `__eoka_pending_requests` page beacon and the page-breaking `Image` naturalHeight hack

#### Binary patcher
- Resolve the real ELF next to a channel wrapper — **fixes Chrome Beta/Dev, which `find_chrome` could not locate at all**
- Stable content-addressed cache (no ~400MB re-copy/re-patch per launch); verification excludes non-rewritten patterns

#### Cookies
- Percent-encode CR/LF and sanitize cookie names (closes a header-injection hole)
- Host-only cookies no longer leak to subdomains (RFC 6265)

#### Page / network capture
- Cache the document node so `find()` no longer invalidates previously-returned element handles
- `goto` waits for load; `wait_for_hidden` handles `display:none`; `\0` escapes as `\x00`
- Retain completed requests, keep the original entry across redirects, non-blocking event emit, O(1) eviction

### Added
- `Session::set_user_agent_full` with client-hint metadata
- `BoxModel::try_center` (fallible center)
- 44 regression/unit tests

### Changed

#### Behavior
- `Element::center` now errors (`ElementNotVisible`) instead of returning `(0.0, 0.0)` and clicking the viewport corner

#### Deprecated
- `BoxModel::center` — use `try_center`

---

## [0.2.1] - 2025-01-26

### Fixed

#### Race Conditions
- `find_by_text_match` and `find_all_by_text` now use unique marker IDs to prevent race conditions between concurrent calls

#### Security
- Added comprehensive JavaScript string escaping (handles `\`, `'`, `"`, `` ` ``, `\n`, `\r`, `${}`)
- `evaluate_in_frame` now uses `Function` constructor instead of `eval()` (CSP-safe)

#### Code Quality
- Extracted `handle_try_result` helper to reduce duplication in try-click methods
- Element inspection methods now use `eval_on_element` helper with focus restoration
- `Element::text()` no longer changes focus as a side effect

#### Bug Fixes
- `bounding_box()` now correctly handles rotated/transformed elements using min/max of quad points
- `debug_screenshot()` now respects `StealthConfig::debug_dir`
- `find_all_by_text` now always cleans up markers, even on errors

### Changed

#### Breaking Changes
- `Element::is_visible()` now returns `Result<bool>` instead of `bool` to distinguish between "not visible" and "network error"
- `PageState` counts (`input_count`, `button_count`, etc.) changed from `i32` to `u32`

#### API Changes
- Added `#[must_use]` attributes to `try_click*`, `exists`, `text_exists`, `is_visible`
- Removed non-functional `find_in_frame` method (use `evaluate_in_frame` instead)

---

## [0.2.0] - 2025-01-26

### Added

#### Text-Based Element Finding
- `find_by_text(text)` - Find element by text content (prioritizes interactive elements)
- `find_by_text_match(text, TextMatch)` - Find with matching strategy (Exact, Contains, StartsWith, EndsWith)
- `find_all_by_text(text)` - Find all elements matching text
- `text_exists(text)` - Check if text exists without error
- `TextMatch` enum for flexible text matching strategies

#### Selector Fallback Chains
- `find_any(&[selectors])` - Try multiple selectors, return first match
- `wait_for_any(&[selectors], timeout)` - Wait for any selector to appear
- `wait_for_any_visible(&[selectors], timeout)` - Wait for any selector to be visible

#### Click Improvements
- `click_by_text(text)` - Click element by visible text
- `human_click_by_text(text)` - Human-like click by text
- `try_click(selector)` - Returns `Ok(false)` instead of error when not found/visible
- `try_click_by_text(text)` - Try-click by text
- `try_human_click(selector)` - Try human-click
- `try_human_click_by_text(text)` - Try human-click by text

#### Form Filling
- `fill(selector, value)` - Clear field and type value
- `human_fill(selector, value)` - Human-like clear and type

#### Wait Helpers
- `wait_for_visible(selector, timeout)` - Wait for element to be rendered and clickable
- `wait_for_text(text, timeout)` - Wait for text to appear
- `wait_for_text_hidden(text, timeout)` - Wait for text to disappear
- `wait_for_url_contains(pattern, timeout)` - Wait for URL pattern
- `wait_for_url_change(timeout)` - Wait for any URL change
- `wait_for_network_idle(idle_ms, timeout)` - Wait for no pending fetch/XHR requests

#### Element Inspection
- `Element::is_visible()` - Check if element has computable box model
- `Element::bounding_box()` - Get element's bounding box
- `Element::get_attribute(name)` - Get attribute value
- `Element::tag_name()` - Get tag name (div, a, button, etc.)
- `Element::is_enabled()` - Check if not disabled
- `Element::is_checked()` - Check checkbox/radio state
- `Element::value()` - Get input value
- `Element::css(property)` - Get computed CSS property
- `Element::scroll_into_view()` - Scroll element into viewport

#### Frame/Iframe Support
- `frames()` - List all frames on page
- `evaluate_in_frame(frame_selector, js)` - Execute JS inside iframe
- `FrameInfo` struct with id, url, name

#### Retry Operations
- `with_retry(attempts, delay_ms, operation)` - Retry flaky operations

#### Debug Helpers
- `debug_screenshot(prefix)` - Save timestamped screenshot
- `debug_state()` - Get `PageState` with element counts
- `PageState` struct with url, title, input/button/link/form counts
- `BoundingBox` struct with x, y, width, height

#### Configuration
- `StealthConfig::debug` - Enable debug mode
- `StealthConfig::debug_dir` - Directory for debug screenshots
- `StealthConfig::debug()` - Preset for debug configuration

### Improved

#### Better Error Messages
- `ElementNotVisible` - "exists in DOM but not rendered" (replaces cryptic CDP errors)
- `ElementNotInteractive` - Element cannot be interacted with
- `RetryExhausted` - After N retry attempts
- `FrameNotFound` - Frame/iframe not found
- `Error::is_not_visible()` - Check if error is visibility-related
- `Error::clarify(selector)` - Convert CDP errors to friendly messages

#### Text Matching
- `find_by_text()` now prioritizes interactive elements (a, button, input) over static elements
- Two-pass search: interactive elements first, then static elements

#### Try-Click Methods
- Now catch both `ElementNotFound` AND CDP box model errors
- Return `Ok(false)` for invisible elements instead of error

### Documentation
- Comprehensive README with real-world login example
- Full API reference for all new methods
- Recipes section for common patterns
- Error handling documentation

---

## [0.1.0] - 2025-01-26

### Added

- Initial release of eoka stealth browser automation library
- Custom CDP transport with built-in command filtering (blocks detectable commands like `Runtime.enable`)
- 15 JavaScript evasion scripts:
  - WebDriver property interception via Proxy
  - Navigator plugins/mimeTypes spoofing
  - Chrome runtime properties
  - Permissions API consistency fixes
  - Battery API on Navigator.prototype
  - WebGL vendor/renderer masking
  - Canvas noise injection
  - Audio fingerprint protection
  - iframe contentWindow fixes
  - Broken image dimension hiding
  - CDP property cleanup
  - WebRTC IP leak prevention
  - Speech synthesis voices spoofing
  - Media devices enumeration
  - Bluetooth API presence
- Human simulation with Bezier curve mouse movements and variable typing delays
- Chrome binary patching to remove automation strings (`$cdc_`, `webdriver`)
- Fingerprint generation (realistic User-Agent, screen dimensions)
- HTTP request capture via CDP Network domain with event streaming (`NetworkWatcher`)
- Screenshot capture with optional annotations (`annotate` feature)
- Session/cookie export for persistence
- GitHub Actions CI workflow (test, clippy, fmt, docs)
- 15 integration tests for CDP commands (browser launch, navigation, screenshots, etc.)

### Features

- `default` - Core functionality
- `annotate` - Screenshot annotations with numbered boxes on interactive elements

### Detection Test Results

- bot.sannysoft.com: All tests pass (including WebDriver New)
- arh.antoinevastel.com/bots/areyouheadless: Not detected
- bot-detector.rebrowser.net: 6/6 tests pass
- browserleaks.com: Clean fingerprint
