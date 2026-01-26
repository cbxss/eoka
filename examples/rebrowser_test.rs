//! Rebrowser bot detector test
//!
//! Run with: cargo run --example rebrowser_test
//! Or visible: cargo run --example rebrowser_test -- --visible

use eoka::{Browser, Result, StealthConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("eoka=info".parse().unwrap())
        )
        .init();

    let visible = std::env::args().any(|a| a == "--visible");

    let config = StealthConfig {
        headless: !visible,
        ..Default::default()
    };

    println!("=== Rebrowser Bot Detector Test ===\n");
    println!("Mode: {}", if visible { "visible" } else { "headless" });

    println!("\nLaunching browser...");
    let browser = Browser::launch_with_config(config).await?;

    let page = browser.new_page("https://bot-detector.rebrowser.net/").await?;

    // Wait for tests to complete
    println!("Waiting for tests to run...");
    page.wait(8000).await;

    // Take screenshot
    let screenshot = page.screenshot().await?;
    std::fs::write("rebrowser.png", screenshot)?;
    println!("Screenshot saved to rebrowser.png");

    // Try to extract test results
    let results: String = page.evaluate(r#"
        (() => {
            // Look for the JSON section
            const h2s = document.querySelectorAll('h2');
            for (const h2 of h2s) {
                if (h2.textContent.includes('JSON')) {
                    // Get the next sibling which should be the pre
                    let next = h2.nextElementSibling;
                    while (next) {
                        if (next.tagName === 'PRE') {
                            return next.textContent;
                        }
                        next = next.nextElementSibling;
                    }
                }
            }

            // Try to get the table results
            const table = document.querySelector('table');
            if (table) {
                const rows = table.querySelectorAll('tr');
                let results = [];
                for (const row of rows) {
                    const cells = row.querySelectorAll('td, th');
                    results.push(Array.from(cells).map(c => c.textContent.trim()).join(' | '));
                }
                return results.join('\n');
            }

            // Last resort - get everything that looks like test results
            const allText = document.body.innerText;
            const lines = allText.split('\n').filter(l =>
                l.includes('Leak') || l.includes('leak') ||
                l.includes('detect') || l.includes('pass') ||
                l.includes('fail') || l.includes('ms')
            );
            return lines.join('\n') || allText.substring(0, 3000);
        })()
    "#).await.unwrap_or_else(|e| format!("Error: {}", e));

    println!("\n--- Detection Results ---\n");
    println!("{}", results);

    browser.close().await?;
    println!("\n=== Test Complete ===");

    Ok(())
}
