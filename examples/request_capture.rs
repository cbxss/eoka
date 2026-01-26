//! Request capture example - demonstrate HTTP request monitoring
//!
//! Run with: cargo run --example request_capture

use eoka::{Browser, Result, StealthConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("eoka=info".parse().unwrap())
        )
        .init();

    println!("=== Eoka Request Capture Example ===\n");

    let config = StealthConfig {
        headless: true,
        ..Default::default()
    };

    println!("Launching browser...");
    let browser = Browser::launch_with_config(config).await?;

    // Create page
    let page = browser.new_page("about:blank").await?;

    // Enable request capture BEFORE navigation
    println!("Enabling request capture...");
    page.enable_request_capture().await?;

    // Navigate to a page that makes API requests
    println!("\nNavigating to httpbin.org...\n");
    page.goto("https://httpbin.org/get?foo=bar&baz=123").await?;
    page.wait(2000).await;

    // Get response body of the page itself
    // Note: To get request IDs you need to listen to network events
    // For now, let's demonstrate the response body API

    // Navigate to a JSON endpoint
    println!("Fetching JSON endpoint...\n");
    page.goto("https://httpbin.org/json").await?;
    page.wait(1000).await;

    // Get the page content (which is the JSON response rendered as text)
    let content = page.text().await?;
    println!("Response content (first 500 chars):");
    println!("{}\n", &content[..content.len().min(500)]);

    // Try a POST request via JavaScript and capture it
    println!("Making a fetch request via JavaScript...");
    let result: serde_json::Value = page.evaluate(r#"
        (async () => {
            const response = await fetch('https://httpbin.org/post', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ message: 'Hello from eoka!' })
            });
            return await response.json();
        })()
    "#).await?;

    println!("POST response:");
    println!("{}\n", serde_json::to_string_pretty(&result)?);

    // Disable request capture
    page.disable_request_capture().await?;

    println!("Closing browser...");
    browser.close().await?;

    println!("\n=== Done ===");
    Ok(())
}
