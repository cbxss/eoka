//! Integration tests for eoka
//!
//! These tests require Chrome to be installed and available.
//! Run with: cargo test --test integration -- --ignored

use eoka::{Browser, StealthConfig};

/// Check if Chrome is available
fn chrome_available() -> bool {
    eoka::stealth::patcher::find_chrome().is_ok()
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_browser_launch() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_browser_launch_visible() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let config = StealthConfig::visible();
    let browser = Browser::launch_with_config(config)
        .await
        .expect("Failed to launch browser");
    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_page_navigation() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    // Navigate to a simple page
    page.goto("data:text/html,<h1>Hello</h1>")
        .await
        .expect("Failed to navigate");

    // Check title
    let content = page.content().await.expect("Failed to get content");
    assert!(content.contains("Hello"));

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_page_title() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto("data:text/html,<title>Test Title</title><body>Content</body>")
        .await
        .expect("Failed to navigate");

    let title = page.title().await.expect("Failed to get title");
    assert_eq!(title, "Test Title");

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_page_url() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto("data:text/html,<h1>Test</h1>")
        .await
        .expect("Failed to navigate");

    let url = page.url().await.expect("Failed to get URL");
    assert!(url.starts_with("data:"));

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_screenshot() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto("data:text/html,<body style='background:red'><h1>Red</h1></body>")
        .await
        .expect("Failed to navigate");

    let png = page.screenshot().await.expect("Failed to take screenshot");

    // Check PNG magic bytes
    assert!(png.len() > 100);
    assert_eq!(&png[0..4], &[0x89, 0x50, 0x4E, 0x47]); // PNG signature

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_screenshot_jpeg() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto("data:text/html,<body style='background:blue'><h1>Blue</h1></body>")
        .await
        .expect("Failed to navigate");

    let jpeg = page
        .screenshot_jpeg(80)
        .await
        .expect("Failed to take screenshot");

    // Check JPEG magic bytes
    assert!(jpeg.len() > 100);
    assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]); // JPEG SOI

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_element_finding() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <button id="btn">Click Me</button>
        <input type="text" class="input" value="test">
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Find by ID
    let btn = page.find("#btn").await.expect("Failed to find button");
    let html = btn.outer_html().await.expect("Failed to get HTML");
    assert!(html.contains("Click Me"));

    // Find by class
    let input = page.find(".input").await.expect("Failed to find input");
    let html = input.outer_html().await.expect("Failed to get HTML");
    assert!(html.contains("type=\"text\""));

    // Find all
    let all = page.find_all("*").await.expect("Failed to find all");
    assert!(all.len() > 2);

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_element_not_found() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto("data:text/html,<div>Simple</div>")
        .await
        .expect("Failed to navigate");

    let result = page.find("#nonexistent").await;
    assert!(result.is_err());

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_evaluate_javascript() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    // Evaluate simple expression
    let result: i32 = page.evaluate("1 + 2").await.expect("Failed to evaluate");
    assert_eq!(result, 3);

    // Evaluate string
    let result: String = page
        .evaluate("'hello' + ' world'")
        .await
        .expect("Failed to evaluate");
    assert_eq!(result, "hello world");

    // Evaluate array
    let result: Vec<i32> = page
        .evaluate("[1, 2, 3]")
        .await
        .expect("Failed to evaluate");
    assert_eq!(result, vec![1, 2, 3]);

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_cookies() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    // Navigate to a real page (cookies need a proper domain)
    page.goto("https://example.com")
        .await
        .expect("Failed to navigate");
    page.wait(1000).await;

    // Set a cookie
    page.set_cookie("test_cookie", "test_value", Some("example.com"), Some("/"))
        .await
        .expect("Failed to set cookie");

    // Get cookies
    let cookies = page.cookies().await.expect("Failed to get cookies");
    let test_cookie = cookies.iter().find(|c| c.name == "test_cookie");
    assert!(test_cookie.is_some());
    assert_eq!(test_cookie.unwrap().value, "test_value");

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_wait_for_element() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    // Page with delayed element
    page.goto(
        r#"data:text/html,
        <script>
            setTimeout(() => {
                document.body.innerHTML = '<div id="delayed">Loaded!</div>';
            }, 100);
        </script>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Wait for element to appear
    let element = page
        .wait_for("#delayed", 5000)
        .await
        .expect("Element not found");
    let html = element.outer_html().await.expect("Failed to get HTML");
    assert!(html.contains("Loaded!"));

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_wait_for_element_timeout() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto("data:text/html,<div>No delayed element</div>")
        .await
        .expect("Failed to navigate");

    // Should timeout
    let result = page.wait_for("#never-exists", 500).await;
    assert!(result.is_err());

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_click_element() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <button id="btn" onclick="this.textContent = 'Clicked!'">Click Me</button>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Click the button
    page.click("#btn").await.expect("Failed to click");

    // Wait a bit for the click to process
    page.wait(100).await;

    // Check that the button text changed
    let content = page.content().await.expect("Failed to get content");
    assert!(content.contains("Clicked!"));

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_type_into_input() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <input type="text" id="input" value="">
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Type into the input
    page.type_into("#input", "Hello World")
        .await
        .expect("Failed to type");

    // Check the value
    let value: String = page
        .evaluate("document.getElementById('input').value")
        .await
        .expect("Failed to evaluate");
    assert_eq!(value, "Hello World");

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_select_dropdown() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <select id="country">
            <option value="us">United States</option>
            <option value="uk">United Kingdom</option>
            <option value="de">Germany</option>
        </select>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Select by value
    page.select("#country", "uk").await.expect("Failed to select");

    let value: String = page
        .evaluate("document.getElementById('country').value")
        .await
        .expect("Failed to evaluate");
    assert_eq!(value, "uk");

    // Select by text
    page.select_by_text("#country", "Germany")
        .await
        .expect("Failed to select by text");

    let value: String = page
        .evaluate("document.getElementById('country').value")
        .await
        .expect("Failed to evaluate");
    assert_eq!(value, "de");

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_select_invalid_option() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <select id="country">
            <option value="us">United States</option>
        </select>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Try to select non-existent value - should error
    let result = page.select("#country", "invalid").await;
    assert!(result.is_err());

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_hover() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    // Simpler data URI without newlines
    page.goto("data:text/html,<div id='target' style='width:100px;height:100px;background:blue'></div><script>window.hovered=false;document.getElementById('target').onmouseenter=()=>{window.hovered=true};</script>")
        .await
        .expect("Failed to navigate");

    // Small delay for page to render
    page.wait(200).await;

    // Hover over the element
    page.hover("#target").await.expect("Failed to hover");

    // Check that hover was triggered
    let hovered: bool = page
        .evaluate("window.hovered")
        .await
        .expect("Failed to evaluate");
    assert!(hovered);

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_press_key() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <input type="text" id="input" autofocus>
        <script>
            window.lastKey = '';
            document.getElementById('input').addEventListener('keydown', (e) => {
                window.lastKey = e.key;
            });
        </script>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Wait for autofocus
    page.wait(100).await;

    // Press Enter
    page.press_key("Enter").await.expect("Failed to press key");

    let key: String = page
        .evaluate("window.lastKey")
        .await
        .expect("Failed to evaluate");
    assert_eq!(key, "Enter");

    // Press Tab
    page.press_tab().await.expect("Failed to press tab");

    let key: String = page
        .evaluate("window.lastKey")
        .await
        .expect("Failed to evaluate");
    assert_eq!(key, "Tab");

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_upload_file() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <input type="file" id="upload">
        <script>
            window.filename = '';
            document.getElementById('upload').addEventListener('change', (e) => {
                window.filename = e.target.files[0]?.name || '';
            });
        </script>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Create a temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("eoka_test_upload.txt");
    std::fs::write(&temp_file, "test content").expect("Failed to write temp file");

    // Upload the file
    page.upload_file("#upload", temp_file.to_str().unwrap())
        .await
        .expect("Failed to upload file");

    // Check that the file was uploaded
    let filename: String = page
        .evaluate("window.filename")
        .await
        .expect("Failed to evaluate");
    assert_eq!(filename, "eoka_test_upload.txt");

    // Clean up
    std::fs::remove_file(&temp_file).ok();

    browser.close().await.expect("Failed to close browser");
}

#[tokio::test]
#[ignore = "requires Chrome"]
async fn test_select_multiple() {
    if !chrome_available() {
        eprintln!("Chrome not found, skipping test");
        return;
    }

    let browser = Browser::launch().await.expect("Failed to launch browser");
    let page = browser
        .new_page("about:blank")
        .await
        .expect("Failed to create page");

    page.goto(
        r#"data:text/html,
        <select id="tags" multiple>
            <option value="rust">Rust</option>
            <option value="async">Async</option>
            <option value="web">Web</option>
        </select>
    "#,
    )
    .await
    .expect("Failed to navigate");

    // Select multiple options
    page.select_multiple("#tags", &["rust", "web"])
        .await
        .expect("Failed to select multiple");

    // Check selected values
    let selected: Vec<String> = page
        .evaluate(
            "Array.from(document.getElementById('tags').selectedOptions).map(o => o.value)",
        )
        .await
        .expect("Failed to evaluate");
    assert_eq!(selected, vec!["rust", "web"]);

    browser.close().await.expect("Failed to close browser");
}
