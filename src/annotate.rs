//! Screenshot Annotation
//!
//! Draws numbered boxes on interactive elements for AI agents.
//! Requires the `annotate` feature to be enabled.

#[cfg(feature = "annotate")]
use image::{Rgba, RgbaImage};

#[cfg(feature = "annotate")]
use imageproc::drawing::draw_hollow_rect_mut;

#[cfg(feature = "annotate")]
use imageproc::rect::Rect;

/// An interactive element on the page
#[derive(Debug, Clone)]
pub struct InteractiveElement {
    /// Element index (1-based)
    pub index: usize,
    /// Element tag name (e.g., "button", "a", "input")
    pub tag: String,
    /// Element text content
    pub text: String,
    /// Bounding box: (x, y, width, height)
    pub bounds: (u32, u32, u32, u32),
    /// Whether the element is clickable
    pub clickable: bool,
    /// Element type attribute (for inputs)
    pub input_type: Option<String>,
    /// Element href (for links)
    pub href: Option<String>,
    /// Element role attribute
    pub role: Option<String>,
}

impl InteractiveElement {
    /// Get the center point of the element
    pub fn center(&self) -> (u32, u32) {
        let (x, y, w, h) = self.bounds;
        (x + w / 2, y + h / 2)
    }

    /// Check if element contains a point
    pub fn contains(&self, px: u32, py: u32) -> bool {
        let (x, y, w, h) = self.bounds;
        px >= x && px <= x + w && py >= y && py <= y + h
    }

    /// Get a short description of the element
    pub fn description(&self) -> String {
        let base = if !self.text.is_empty() {
            let truncated = if self.text.len() > 30 {
                format!("{}...", &self.text[..27])
            } else {
                self.text.clone()
            };
            format!("{}: \"{}\"", self.tag, truncated)
        } else if let Some(ref href) = self.href {
            format!("{}: {}", self.tag, href)
        } else if let Some(ref role) = self.role {
            format!("{} [{}]", self.tag, role)
        } else {
            self.tag.clone()
        };

        if let Some(ref input_type) = self.input_type {
            format!("{} (type={})", base, input_type)
        } else {
            base
        }
    }
}

/// Annotation configuration
#[derive(Debug, Clone)]
pub struct AnnotationConfig {
    /// Box line color (RGBA)
    pub box_color: [u8; 4],
    /// Label background color (RGBA)
    pub label_bg_color: [u8; 4],
    /// Label text color (RGBA)
    pub label_text_color: [u8; 4],
    /// Box line thickness
    pub line_thickness: u32,
    /// Label font size
    pub font_size: f32,
    /// Label padding
    pub label_padding: u32,
}

impl Default for AnnotationConfig {
    fn default() -> Self {
        Self {
            box_color: [255, 0, 0, 200],         // Red with some transparency
            label_bg_color: [255, 0, 0, 220],    // Red background
            label_text_color: [255, 255, 255, 255], // White text
            line_thickness: 2,
            font_size: 14.0,
            label_padding: 2,
        }
    }
}

/// Annotate a screenshot with numbered boxes
#[cfg(feature = "annotate")]
pub fn annotate_screenshot(
    png_data: &[u8],
    elements: &[InteractiveElement],
    config: &AnnotationConfig,
) -> Result<Vec<u8>, AnnotationError> {
    // Load image from PNG data
    let img = image::load_from_memory(png_data)
        .map_err(|e| AnnotationError::ImageLoad(e.to_string()))?;
    let mut rgba = img.to_rgba8();

    let box_color = Rgba(config.box_color);
    let label_bg = Rgba(config.label_bg_color);
    let label_text = Rgba(config.label_text_color);

    for element in elements {
        let (x, y, w, h) = element.bounds;

        // Draw bounding box
        if w > 0 && h > 0 {
            let rect = Rect::at(x as i32, y as i32).of_size(w, h);
            draw_hollow_rect_mut(&mut rgba, rect, box_color);
        }

        // Draw label with index number
        let label = format!("{}", element.index);
        let char_width = 8u32;
        let char_height = 12u32;
        let label_width = (label.len() as u32 * char_width) + config.label_padding * 2;
        let label_height = char_height + config.label_padding * 2;

        // Position label at top-left of box
        let label_x = x;
        let label_y = if y >= label_height {
            y - label_height
        } else {
            y
        };

        // Draw label background
        draw_filled_rect(&mut rgba, label_x, label_y, label_width, label_height, label_bg);

        // Draw label text using simple pixel font
        let text_x = label_x + config.label_padding;
        let text_y = label_y + config.label_padding;
        draw_number_text(&mut rgba, text_x, text_y, &label, label_text);
    }

    // Encode back to PNG
    let mut output = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut output, image::ImageFormat::Png)
        .map_err(|e| AnnotationError::ImageEncode(e.to_string()))?;

    Ok(output.into_inner())
}

/// Simple 5x7 pixel font for digits 0-9
#[cfg(feature = "annotate")]
const DIGIT_PATTERNS: [[u8; 7]; 10] = [
    // 0
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    // 1
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    // 2
    [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
    // 3
    [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
    // 4
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    // 5
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    // 6
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    // 7
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    // 8
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    // 9
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

/// Draw a number string using simple pixel font
#[cfg(feature = "annotate")]
fn draw_number_text(img: &mut RgbaImage, x: u32, y: u32, text: &str, color: Rgba<u8>) {
    let mut cx = x;
    for ch in text.chars() {
        if let Some(digit) = ch.to_digit(10) {
            let pattern = &DIGIT_PATTERNS[digit as usize];
            for (row, &bits) in pattern.iter().enumerate() {
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 == 1 {
                        let px = cx + col;
                        let py = y + row as u32;
                        if px < img.width() && py < img.height() {
                            img.put_pixel(px, py, color);
                        }
                    }
                }
            }
            cx += 8; // char width + spacing
        }
    }
}

/// Draw a filled rectangle
#[cfg(feature = "annotate")]
fn draw_filled_rect(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: Rgba<u8>,
) {
    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px < img.width() && py < img.height() {
                img.put_pixel(px, py, color);
            }
        }
    }
}

/// Annotation error
#[derive(Debug)]
pub enum AnnotationError {
    ImageLoad(String),
    ImageEncode(String),
    FontLoad,
}

impl std::fmt::Display for AnnotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnnotationError::ImageLoad(e) => write!(f, "Failed to load image: {}", e),
            AnnotationError::ImageEncode(e) => write!(f, "Failed to encode image: {}", e),
            AnnotationError::FontLoad => write!(f, "Failed to load font"),
        }
    }
}

impl std::error::Error for AnnotationError {}

/// Stub for when annotate feature is not enabled
#[cfg(not(feature = "annotate"))]
pub fn annotate_screenshot(
    _png_data: &[u8],
    _elements: &[InteractiveElement],
    _config: &AnnotationConfig,
) -> Result<Vec<u8>, AnnotationError> {
    Err(AnnotationError::ImageLoad(
        "annotate feature not enabled - add `annotate` feature to Cargo.toml".to_string()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interactive_element_center() {
        let element = InteractiveElement {
            index: 1,
            tag: "button".to_string(),
            text: "Click me".to_string(),
            bounds: (100, 100, 200, 50),
            clickable: true,
            input_type: None,
            href: None,
            role: None,
        };

        let (cx, cy) = element.center();
        assert_eq!(cx, 200); // 100 + 200/2
        assert_eq!(cy, 125); // 100 + 50/2
    }

    #[test]
    fn test_interactive_element_contains() {
        let element = InteractiveElement {
            index: 1,
            tag: "button".to_string(),
            text: "Click me".to_string(),
            bounds: (100, 100, 200, 50),
            clickable: true,
            input_type: None,
            href: None,
            role: None,
        };

        assert!(element.contains(150, 120));
        assert!(element.contains(100, 100)); // Edge
        assert!(element.contains(300, 150)); // Edge
        assert!(!element.contains(50, 50));  // Outside
        assert!(!element.contains(350, 120)); // Outside
    }

    #[test]
    fn test_interactive_element_description() {
        let button = InteractiveElement {
            index: 1,
            tag: "button".to_string(),
            text: "Submit".to_string(),
            bounds: (0, 0, 100, 30),
            clickable: true,
            input_type: None,
            href: None,
            role: None,
        };
        assert_eq!(button.description(), "button: \"Submit\"");

        let link = InteractiveElement {
            index: 2,
            tag: "a".to_string(),
            text: "".to_string(),
            bounds: (0, 0, 100, 30),
            clickable: true,
            input_type: None,
            href: Some("https://example.com".to_string()),
            role: None,
        };
        assert_eq!(link.description(), "a: https://example.com");

        let input = InteractiveElement {
            index: 3,
            tag: "input".to_string(),
            text: "".to_string(),
            bounds: (0, 0, 100, 30),
            clickable: true,
            input_type: Some("email".to_string()),
            href: None,
            role: Some("textbox".to_string()),
        };
        assert_eq!(input.description(), "input [textbox] (type=email)");
    }
}
