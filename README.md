# eoka

[![crates.io](https://img.shields.io/crates/v/eoka.svg)](https://crates.io/crates/eoka)
[![docs.rs](https://docs.rs/eoka/badge.svg)](https://docs.rs/eoka)
[![CI](https://github.com/cbxss/eoka/actions/workflows/ci.yml/badge.svg)](https://github.com/cbxss/eoka/actions/workflows/ci.yml)

Stealth browser automation. Passes bot detection without the bloat.

## Requirements

Chrome or Chromium must be installed. eoka launches and controls a real browser instance via CDP.

## Install

```toml
[dependencies]
eoka = "0.3"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick Start

```rust
use eoka::{Browser, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let browser = Browser::launch().await?;
    let page = browser.new_page("https://example.com").await?;

    page.human_click("#button").await?;
    page.human_type("#input", "hello").await?;

    let png = page.screenshot().await?;
    std::fs::write("screenshot.png", png)?;

    browser.close().await?;
    Ok(())
}
```

## Real-World Example: Login Flow

```rust
use eoka::{Browser, Result, StealthConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Launch visible browser for debugging
    let config = StealthConfig {
        headless: false,
        ..Default::default()
    };
    let browser = Browser::launch_with_config(config).await?;
    let page = browser.new_page("https://example.com/login").await?;

    // Dismiss cookie banner if present (won't error if missing)
    page.try_click_by_text("Accept Cookies").await?;

    // Click sign-in link by its text
    page.human_click_by_text("Sign In").await?;

    // Wait for login form to be visible and clickable
    page.wait_for_visible("#email", 10_000).await?;

    // Fill form fields (clears existing content first)
    page.human_fill("#email", "user@example.com").await?;
    page.human_fill("#password", "secret123").await?;

    // Submit
    page.human_click_by_text("Log In").await?;

    // Wait for success indicator
    page.wait_for_text("Welcome back", 15_000).await?;

    browser.close().await?;
    Ok(())
}
```

## What it does

Patches Chrome binary to remove `$cdc_` and `webdriver` strings. Injects 15 evasion scripts before page load. Blocks detectable CDP commands (`Runtime.enable`, `Debugger.enable`, etc.) at the transport layer. Simulates human mouse movement with Bezier curves.

Passes: sannysoft, rebrowser bot detector (6/6), areyouheadless, browserleaks

Partial: creepjs (33% trust score - it's good at what it does)

## API Reference

### Finding Elements

```rust
// By CSS selector
let elem = page.find("#login-button").await?;
let elems = page.find_all(".item").await?;

// By text content (case-insensitive, prioritizes links/buttons)
let btn = page.find_by_text("Sign In").await?;
let items = page.find_all_by_text("Add to Cart").await?;

// Fallback chains - try multiple selectors
let email = page.find_any(&["#email", "input[type='email']", "[name='email']"]).await?;

// Check existence without error
if page.exists("#popup").await { /* ... */ }
if page.text_exists("Error").await { /* ... */ }
```

### Clicking

```rust
// By selector
page.click("#button").await?;
page.human_click("#button").await?;  // with mouse movement

// By text
page.click_by_text("Submit").await?;
page.human_click_by_text("Submit").await?;

// Try-click: returns Ok(false) instead of error when not found
if page.try_click(".optional-popup").await? {
    println!("Popup dismissed");
}
page.try_click_by_text("Accept").await?;
page.try_human_click("#maybe-exists").await?;
```

### Form Filling

```rust
// fill() clears existing content before typing
page.fill("#email", "user@example.com").await?;
page.human_fill("#password", "secret").await?;  // with natural delays

// type_into() doesn't clear first (appends)
page.type_into("#search", "query").await?;
page.human_type("#search", "query").await?;
```

### Waiting

```rust
// Wait for element by selector (in DOM)
page.wait_for("#results", 10_000).await?;
page.wait_for_hidden(".loading", 5_000).await?;

// Wait for element to be VISIBLE and clickable (recommended before interaction)
page.wait_for_visible("#email", 10_000).await?;

// Wait for any of multiple selectors
page.wait_for_any(&["#success", ".error-message"], 10_000).await?;

// Wait for element by text
page.wait_for_text("Success!", 10_000).await?;

// Wait for URL changes
page.wait_for_url_contains("dashboard", 10_000).await?;
page.wait_for_url_change(10_000).await?;

// Wait for network to be idle (no pending requests)
page.wait_for_network_idle(500, 30_000).await?;  // 500ms idle, 30s timeout

// Fixed delay (use sparingly)
page.wait(1000).await;
```

### Element Inspection

```rust
let elem = page.find("#my-button").await?;

// Visibility
elem.is_visible().await?;  // Result<bool> - can we click it?
elem.bounding_box().await;  // Option<BoundingBox>

// Attributes
elem.get_attribute("href").await?;  // Option<String>
elem.tag_name().await?;  // "button", "a", "input", etc.

// State
elem.is_enabled().await?;  // not disabled
elem.is_checked().await?;  // for checkboxes/radios
elem.value().await?;  // input value

// Styling
elem.css("color").await?;  // computed CSS value

// Actions
elem.scroll_into_view().await?;
```

### Page Info

```rust
let url = page.url().await?;
let title = page.title().await?;
let html = page.content().await?;
let text = page.text().await?;
let png = page.screenshot().await?;

// Debug info
let state = page.debug_state().await?;
println!("URL: {}, Inputs: {}, Buttons: {}", state.url, state.input_count, state.button_count);

// Debug screenshot with timestamp
let filename = page.debug_screenshot("step1").await?;
```

### JavaScript

```rust
// Evaluate and get result
let count: i32 = page.evaluate("document.querySelectorAll('li').length").await?;

// Execute without return value
page.execute("window.scrollTo(0, 1000)").await?;

// Execute inside an iframe
let title: String = page.evaluate_in_frame("iframe#widget", "document.title").await?;
```

### Frames/Iframes

```rust
// List all frames
let frames = page.frames().await?;
for frame in frames {
    println!("Frame: {} - {}", frame.id, frame.url);
}

// Execute JavaScript inside iframe
let count: i32 = page.evaluate_in_frame("iframe.login-widget", "document.forms.length").await?;
```

### Retry Operations

```rust
// Retry flaky operations
page.with_retry(3, 500, || async {
    page.human_click("#sometimes-slow-button").await
}).await?;
```

### Multi-Tab

```rust
// Create multiple tabs
let page1 = browser.new_page("https://a.com").await?;
let page2 = browser.new_page("https://b.com").await?;

// List all open tabs
for tab in browser.tabs().await? {
    println!("{}: {}", tab.id, tab.url);
}

// Get page's tab ID
let id = page1.target_id();

// Focus a tab
browser.activate_tab(page1.target_id()).await?;

// Close a specific tab
browser.close_tab(page2.target_id()).await?;
```

### File Uploads

```rust
// Single file
page.upload_file("input[type='file']", "/path/to/document.pdf").await?;

// Multiple files
page.upload_files("input[type='file']", &["/path/to/a.pdf", "/path/to/b.pdf"]).await?;
```

### Select / Dropdowns

```rust
// By value
page.select("#country", "US").await?;

// By visible text
page.select_by_text("#country", "United States").await?;

// Multi-select
page.select_multiple("#tags", &["rust", "async", "web"]).await?;
```

### Hover (Reveal Menus)

```rust
// Hover to reveal dropdown menu
page.hover("#menu-trigger").await?;
page.click("#submenu-item").await?;

// Human-like hover
page.human_hover("#tooltip-trigger").await?;
```

### Keyboard Shortcuts

```rust
// Single keys
page.press_key("Enter").await?;
page.press_key("Tab").await?;
page.press_key("Escape").await?;

// With modifiers
page.press_key("Ctrl+A").await?;      // Select all
page.press_key("Cmd+C").await?;       // Copy (Mac)
page.press_key("Ctrl+Shift+S").await?; // Save as

// Convenience methods
page.select_all().await?;  // Ctrl+A / Cmd+A
page.copy().await?;        // Ctrl+C / Cmd+C
page.paste().await?;       // Ctrl+V / Cmd+V
```

## Recipes

### Handle Cookie Banners

```rust
// Try multiple common selectors
for selector in ["#accept-cookies", ".cookie-accept", "[data-consent='accept']"] {
    if page.try_click(selector).await? {
        break;
    }
}
// Or by text
page.try_click_by_text("Accept All").await?;
```

### Wait for Page After Click

```rust
// Wait for URL to change
page.human_click_by_text("Sign In").await?;
page.wait_for_url_contains("login", 10_000).await?;

// Or wait for specific content
page.human_click_by_text("Submit").await?;
page.wait_for_text("Success", 10_000).await?;
```

### Handle Dynamic/AJAX Pages

```rust
page.human_click_by_text("Load More").await?;
page.wait_for_network_idle(500, 30_000).await?;  // Wait for XHR to complete
```

### Fill Multi-Step Form

```rust
// Step 1: Personal info
page.human_fill("#first-name", "John").await?;
page.human_fill("#last-name", "Doe").await?;
page.human_click_by_text("Continue").await?;

// Step 2: Wait for next section to be visible, then fill
page.wait_for_visible("#address", 5_000).await?;
page.human_fill("#address", "123 Main St").await?;
page.human_click_by_text("Submit").await?;
```

### Handle Login With Redirect

```rust
let page = browser.new_page("https://app.example.com/dashboard").await?;

// If redirected to login
if page.text_exists("Sign In").await {
    page.human_fill("#email", "user@example.com").await?;
    page.human_fill("#password", "secret").await?;
    page.human_click_by_text("Sign In").await?;
    page.wait_for_url_change(10_000).await?;
}

// Now on dashboard
page.wait_for_text("Dashboard", 10_000).await?;
```

### Robust Element Selection

```rust
// Use fallback chains for inconsistent pages
let email_input = page.find_any(&[
    "#email",
    "input[type='email']",
    "input[name='email']",
    "[placeholder*='email']",
]).await?;

// Or wait for any to appear
page.wait_for_any(&["#login-form", "#sso-redirect"], 10_000).await?;
```

## Config

```rust
let config = StealthConfig {
    headless: false,        // visible browser
    patch_binary: true,     // patch chrome (default)
    human_mouse: true,      // bezier curves (default)
    human_typing: true,     // variable delays (default)
    debug: true,            // enable debug logging
    ..Default::default()
};
let browser = Browser::launch_with_config(config).await?;

// Or use presets
let browser = Browser::launch_with_config(StealthConfig::visible()).await?;
let browser = Browser::launch_with_config(StealthConfig::debug()).await?;
```

## Error Handling

Eoka provides descriptive error messages:

```rust
// Element not visible (instead of cryptic CDP error)
// Error: Element not visible: '#hidden-btn' exists in DOM but is not rendered

// Timeout with context
// Error: Timeout: Element '#results' not visible within 10000ms

// Retry exhausted
// Error: Retry exhausted after 3 attempts: Element not found: #flaky-element
```

## Examples

```bash
cargo run --example basic
cargo run --example detection_test
cargo run --example detection_test -- --visible
```

## How it works

~5K lines of Rust. No chromiumoxide, no puppeteer-extra. Hand-written CDP types for the ~30 commands we actually need.

```
src/
├── cdp/           # websocket transport, command filtering
├── stealth/       # evasions, binary patcher, human simulation
├── browser.rs     # chrome launcher
├── page.rs        # page api
└── session.rs     # cookie export
```

The key insight: most detection comes from CDP commands leaking (`Runtime.enable` fires `consoleAPICalled` events that pages can detect). We block those at the transport layer and define navigator properties on the prototype instead of the instance.

## Ecosystem

Higher-level tools built on eoka:

| Crate | Description |
|-------|-------------|
| [eoka-agent](https://crates.io/crates/eoka-agent) | AI agent layer with MCP server — observe/act loop, annotated screenshots, live element targeting |
| [eoka-runner](https://crates.io/crates/eoka-runner) | Config-based automation — define flows in YAML, run deterministically |

## License

MIT
