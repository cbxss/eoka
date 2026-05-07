//! Attach to a Chrome that's already running with `--remote-debugging-port=9222`.
//!
//! ```bash
//! google-chrome --remote-debugging-port=9222 &
//! cargo run --example connect_existing
//! ```
//!
//! Lists open tabs, attaches to the first one, and saves a screenshot.

use eoka::cdp::discover;
use eoka::Browser;

#[tokio::main]
async fn main() -> eoka::Result<()> {
    tracing_subscriber::fmt::init();

    let port: u16 = std::env::args()
        .nth(1)
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9222);

    println!("Discovering Chrome on 127.0.0.1:{}...", port);
    let pages = discover::discover_pages("127.0.0.1", port)?;
    let tabs: Vec<_> = pages.iter().filter(|p| p.kind == "page").collect();
    println!("Found {} tab(s):", tabs.len());
    for (i, t) in tabs.iter().enumerate() {
        println!("  {}: {} — {}", i, t.title, t.url);
    }

    let first = tabs
        .first()
        .ok_or_else(|| eoka::Error::CdpSimple("No open page tabs".into()))?;

    let browser = Browser::connect_port(port).await?;
    let page = browser.attach_page(&first.id).await?;

    println!("URL:   {}", page.url().await?);
    println!("Title: {}", page.title().await?);

    let png = page.screenshot().await?;
    std::fs::write("connected.png", png)?;
    println!("Wrote connected.png ({} bytes)", std::fs::metadata("connected.png")?.len());

    // Note: we do NOT call browser.close() — that would send a Browser.close
    // command to the user's real Chrome and kill it. Just drop the handle.
    Ok(())
}
