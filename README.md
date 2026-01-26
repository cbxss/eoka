# eoka

Stealth browser automation. Passes bot detection without the bloat.

## Install

```toml
[dependencies]
eoka = { git = "https://github.com/cbxss/eoka" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Usage

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

## What it does

Patches Chrome binary to remove `$cdc_` and `webdriver` strings. Injects 15 evasion scripts before page load. Blocks detectable CDP commands (`Runtime.enable`, `Debugger.enable`, etc.) at the transport layer. Simulates human mouse movement with Bezier curves.

Passes: sannysoft, rebrowser bot detector (6/6), areyouheadless, browserleaks

Partial: creepjs (33% trust score - it's good at what it does)

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

## Config

```rust
let config = StealthConfig {
    headless: false,        // visible browser
    patch_binary: true,     // patch chrome (default)
    human_mouse: true,      // bezier curves (default)
    human_typing: true,     // variable delays (default)
    ..Default::default()
};
let browser = Browser::launch_with_config(config).await?;
```

## License

MIT
