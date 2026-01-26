//! Detection test - verify stealth features work
//!
//! Run with: cargo run --example detection_test
//! Or visible: cargo run --example detection_test -- --visible

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

    // Check for --visible flag
    let visible = std::env::args().any(|a| a == "--visible");

    let config = StealthConfig {
        headless: !visible,
        ..Default::default()
    };

    println!("=== Eoka Detection Test ===\n");
    println!("Mode: {}", if visible { "visible" } else { "headless" });
    println!();

    println!("Launching browser...");
    let browser = Browser::launch_with_config(config).await?;

    // Test 1: bot.sannysoft.com
    println!("\n--- Test 1: bot.sannysoft.com ---\n");
    let page = browser.new_page("https://bot.sannysoft.com").await?;
    page.wait(3000).await;

    let screenshot = page.screenshot().await?;
    std::fs::write("sannysoft.png", screenshot)?;
    println!("Screenshot saved to sannysoft.png");

    // Check for WebDriver detection
    let webdriver_check: String = page.evaluate(
        r#"
        (() => {
            const results = [];
            results.push('navigator.webdriver: ' + navigator.webdriver);
            results.push("'webdriver' in navigator: " + ('webdriver' in navigator));
            if (typeof Notification !== 'undefined') {
                results.push('Notification.permission: ' + Notification.permission);
            }
            const markers = ['$cdc_', 'webdriver', '__webdriver'];
            let foundMarkers = [];
            for (const key of Object.keys(window)) {
                for (const marker of markers) {
                    if (key.includes(marker)) {
                        foundMarkers.push(key);
                    }
                }
            }
            results.push('Automation markers found: ' + (foundMarkers.length > 0 ? foundMarkers.join(', ') : 'none'));
            results.push('chrome.runtime exists: ' + !!(window.chrome && window.chrome.runtime));
            results.push('navigator.plugins.length: ' + navigator.plugins.length);
            return results.join('\n');
        })()
        "#
    ).await?;

    println!("Detection checks:");
    for line in webdriver_check.lines() {
        println!("  {}", line);
    }

    // Test 2: areyouheadless
    println!("\n--- Test 2: arh.antoinevastel.com ---\n");
    page.goto("https://arh.antoinevastel.com/bots/areyouheadless").await?;
    page.wait(2000).await;

    let screenshot = page.screenshot().await?;
    std::fs::write("areyouheadless.png", screenshot)?;
    println!("Screenshot saved to areyouheadless.png");

    let result_text = page.text().await?;
    if result_text.to_lowercase().contains("you are not") {
        println!("Result: PASS - Not detected as headless!");
    } else if result_text.to_lowercase().contains("you are") {
        println!("Result: FAIL - Detected as headless");
    } else {
        println!("Result text: {}", &result_text[..result_text.len().min(500)]);
    }

    // Test 3: Incolumitas Bot Detector
    println!("\n--- Test 3: bot.incolumitas.com ---\n");
    page.goto("https://bot.incolumitas.com").await?;
    page.wait(5000).await; // This one takes longer

    let screenshot = page.screenshot().await?;
    std::fs::write("incolumitas.png", screenshot)?;
    println!("Screenshot saved to incolumitas.png");

    // Try to get the detection results
    let incolumitas_result: String = page.evaluate(
        r#"
        (() => {
            // Look for the results
            const results = document.querySelector('.bot-detection-result, #detection-result, .result');
            if (results) return results.innerText;

            // Try to find any score or verdict
            const text = document.body.innerText;
            const lines = text.split('\n').filter(l =>
                l.includes('Score') || l.includes('Bot') || l.includes('Human') ||
                l.includes('Detection') || l.includes('Result')
            );
            return lines.slice(0, 5).join('\n') || 'Results still loading...';
        })()
        "#
    ).await.unwrap_or_else(|_| "Could not get results".to_string());
    println!("Results:\n{}", incolumitas_result);

    // Test 4: CreepJS (comprehensive fingerprinting)
    println!("\n--- Test 4: CreepJS ---\n");
    page.goto("https://abrahamjuliot.github.io/creepjs/").await?;
    page.wait(8000).await; // CreepJS takes a while to run all tests

    let screenshot = page.screenshot().await?;
    std::fs::write("creepjs.png", screenshot)?;
    println!("Screenshot saved to creepjs.png");

    let creepjs_result: String = page.evaluate(
        r#"
        (() => {
            // Get the trust score
            const trustScore = document.querySelector('.trust-score, [class*="trust"], [class*="score"]');
            if (trustScore) return 'Trust Score: ' + trustScore.innerText;

            // Try to find grade
            const grade = document.querySelector('.grade, [class*="grade"]');
            if (grade) return 'Grade: ' + grade.innerText;

            return 'Check screenshot for detailed results';
        })()
        "#
    ).await.unwrap_or_else(|_| "Check screenshot".to_string());
    println!("{}", creepjs_result);

    // Test 5: Pixelscan
    println!("\n--- Test 5: Pixelscan ---\n");
    page.goto("https://pixelscan.net/").await?;
    page.wait(5000).await;

    let screenshot = page.screenshot().await?;
    std::fs::write("pixelscan.png", screenshot)?;
    println!("Screenshot saved to pixelscan.png");

    let pixelscan_result: String = page.evaluate(
        r#"
        (() => {
            // Look for the verdict
            const verdict = document.querySelector('.verdict, .result, .status, [class*="consistent"]');
            if (verdict) return verdict.innerText;

            // Check for any warnings
            const warnings = document.querySelectorAll('.warning, .inconsistent, [class*="warning"]');
            if (warnings.length > 0) {
                return 'Warnings found: ' + warnings.length;
            }

            return 'Check screenshot for detailed results';
        })()
        "#
    ).await.unwrap_or_else(|_| "Check screenshot".to_string());
    println!("{}", pixelscan_result);

    // Test 6: BrowserLeaks WebRTC
    println!("\n--- Test 6: BrowserLeaks ---\n");
    page.goto("https://browserleaks.com/javascript").await?;
    page.wait(3000).await;

    let screenshot = page.screenshot().await?;
    std::fs::write("browserleaks.png", screenshot)?;
    println!("Screenshot saved to browserleaks.png");

    let browserleaks_result: String = page.evaluate(
        r#"
        (() => {
            // Check for webdriver detection
            const rows = document.querySelectorAll('tr');
            for (const row of rows) {
                const text = row.innerText.toLowerCase();
                if (text.includes('webdriver')) {
                    return 'WebDriver row: ' + row.innerText.replace(/\s+/g, ' ').trim();
                }
            }
            return 'Check screenshot for detailed results';
        })()
        "#
    ).await.unwrap_or_else(|_| "Check screenshot".to_string());
    println!("{}", browserleaks_result);

    // Clean up
    println!("\nClosing browser...");
    browser.close().await?;

    println!("\n=== Test Complete ===");
    println!("\nScreenshots saved:");
    println!("  - sannysoft.png");
    println!("  - areyouheadless.png");
    println!("  - incolumitas.png");
    println!("  - creepjs.png");
    println!("  - pixelscan.png");
    println!("  - browserleaks.png");

    Ok(())
}
