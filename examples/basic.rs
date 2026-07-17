//! Basic usage example for eoka
//!
//! Run with: cargo run --example basic
//! Or visible: cargo run --example basic -- --visible

use eoka::{Browser, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let visible = std::env::args().any(|a| a == "--visible");

    println!("Mode: {}", if visible { "visible" } else { "headless" });
    println!("Launching browser...");
    let browser = if visible {
        Browser::launch_visible().await?
    } else {
        Browser::launch().await?
    };

    // Get browser version
    let version = browser.version().await?;
    println!("Browser version: {}", version);

    // Create a new page and navigate to example.com
    println!("Creating page and navigating to example.com...");
    let page = browser.new_page("https://example.com").await?;

    // Wait for page to load
    page.wait(1000).await;

    // Get page info
    let url = page.url().await?;
    let title = page.title().await?;
    println!("URL: {}", url);
    println!("Title: {}", title);

    // Get page text
    let text = page.text().await?;
    println!(
        "Page text (first 200 chars): {}",
        &text[..text.len().min(200)]
    );

    // Take a screenshot
    let screenshot = page.screenshot().await?;
    std::fs::write("example_screenshot.png", screenshot)?;
    println!("Screenshot saved to example_screenshot.png");

    // Clean up
    println!("Closing browser...");
    browser.close().await?;

    println!("Done!");
    Ok(())
}
