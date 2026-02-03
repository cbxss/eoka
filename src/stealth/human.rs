//! Human-like browser interactions
//!
//! Simulates realistic mouse movements and typing patterns to avoid
//! behavior-based bot detection.

use rand::Rng;
use smallvec::SmallVec;
use std::cell::RefCell;
use std::time::Duration;
use tokio::time::sleep;

use crate::cdp::{KeyEventType, MouseButton, MouseEventType, Session};
use crate::error::Result;

// Thread-local RNG
thread_local! {
    static RNG: RefCell<rand::rngs::ThreadRng> = RefCell::new(rand::rng());
}

/// Speed mode for human simulation
#[derive(Debug, Clone, Copy, Default)]
pub enum HumanSpeed {
    /// Fast mode - minimal delays
    Fast,
    /// Normal mode - balanced
    #[default]
    Normal,
    /// Slow mode - maximum realism
    Slow,
}

impl HumanSpeed {
    fn mouse_points(&self, distance: f64) -> usize {
        match self {
            HumanSpeed::Fast => (distance / 50.0).clamp(3.0, 10.0) as usize,
            HumanSpeed::Normal => (distance / 10.0).clamp(10.0, 50.0) as usize,
            HumanSpeed::Slow => (distance / 5.0).clamp(20.0, 100.0) as usize,
        }
    }

    fn move_delay_ms(&self) -> (u64, u64) {
        match self {
            HumanSpeed::Fast => (1, 5),
            HumanSpeed::Normal => (5, 25),
            HumanSpeed::Slow => (10, 50),
        }
    }

    fn type_delay_ms(&self) -> (u64, u64) {
        match self {
            HumanSpeed::Fast => (10, 30),
            HumanSpeed::Normal => (50, 150),
            HumanSpeed::Slow => (100, 300),
        }
    }
}

fn random_range(min: u64, max: u64) -> u64 {
    RNG.with(|rng| rng.borrow_mut().random_range(min..max))
}

fn random_f64_range(min: f64, max: f64) -> f64 {
    RNG.with(|rng| rng.borrow_mut().random_range(min..max))
}

fn random_bool(probability: f64) -> bool {
    RNG.with(|rng| rng.borrow_mut().random_bool(probability))
}

/// Point type
type Point = (f64, f64);

/// Stack-allocated storage for typical mouse paths
type PointVec = SmallVec<[Point; 64]>;

/// Generate Bezier curve for natural mouse movement
#[inline]
fn bezier_curve(start: Point, end: Point, num_points: usize) -> PointVec {
    let num_points = num_points.max(2);

    let cp1 = (
        start.0 + (end.0 - start.0) * random_f64_range(0.2, 0.4) + random_f64_range(-50.0, 50.0),
        start.1 + (end.1 - start.1) * random_f64_range(0.0, 0.3) + random_f64_range(-50.0, 50.0),
    );
    let cp2 = (
        start.0 + (end.0 - start.0) * random_f64_range(0.6, 0.8) + random_f64_range(-50.0, 50.0),
        start.1 + (end.1 - start.1) * random_f64_range(0.7, 1.0) + random_f64_range(-50.0, 50.0),
    );

    let mut points = PointVec::new();

    for i in 0..num_points {
        let t = i as f64 / (num_points - 1) as f64;
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        let x = mt3 * start.0 + 3.0 * mt2 * t * cp1.0 + 3.0 * mt * t2 * cp2.0 + t3 * end.0;
        let y = mt3 * start.1 + 3.0 * mt2 * t * cp1.1 + 3.0 * mt * t2 * cp2.1 + t3 * end.1;

        points.push((x, y));
    }

    points
}

/// Human-like interaction helpers
pub struct Human<'a> {
    session: &'a Session,
    speed: HumanSpeed,
}

impl<'a> Human<'a> {
    /// Create a new Human helper
    pub fn new(session: &'a Session) -> Self {
        Self {
            session,
            speed: HumanSpeed::Normal,
        }
    }

    /// Set the speed mode
    pub fn with_speed(mut self, speed: HumanSpeed) -> Self {
        self.speed = speed;
        self
    }

    /// Move mouse to target position with human-like Bezier curve
    pub async fn move_to(&self, target_x: f64, target_y: f64) -> Result<()> {
        // Start from random position
        let start_x = random_f64_range(100.0, 800.0);
        let start_y = random_f64_range(100.0, 600.0);

        let distance = ((target_x - start_x).powi(2) + (target_y - start_y).powi(2)).sqrt();
        let num_points = self.speed.mouse_points(distance);
        let (min_delay, max_delay) = self.speed.move_delay_ms();

        let path = bezier_curve((start_x, start_y), (target_x, target_y), num_points);

        // Move through path
        for (x, y) in path {
            self.session
                .dispatch_mouse_event(MouseEventType::MouseMoved, x, y, None, None)
                .await?;
            sleep(Duration::from_millis(random_range(min_delay, max_delay))).await;
        }

        Ok(())
    }

    /// Move mouse to target and click
    pub async fn move_and_click(&self, target_x: f64, target_y: f64) -> Result<()> {
        self.move_to(target_x, target_y).await?;

        // Small delay before click
        sleep(Duration::from_millis(random_range(50, 150))).await;

        // Click with slight jitter
        let click_x = target_x + random_f64_range(-2.0, 2.0);
        let click_y = target_y + random_f64_range(-2.0, 2.0);

        // Mouse down
        self.session
            .dispatch_mouse_event(
                MouseEventType::MousePressed,
                click_x,
                click_y,
                Some(MouseButton::Left),
                Some(1),
            )
            .await?;

        sleep(Duration::from_millis(random_range(50, 120))).await;

        // Mouse up
        self.session
            .dispatch_mouse_event(
                MouseEventType::MouseReleased,
                click_x,
                click_y,
                Some(MouseButton::Left),
                Some(1),
            )
            .await?;

        // Small delay after click
        sleep(Duration::from_millis(random_range(30, 100))).await;

        Ok(())
    }

    /// Type text with human-like timing
    pub async fn type_text(&self, text: &str) -> Result<()> {
        let (min_delay, max_delay) = self.speed.type_delay_ms();

        for ch in text.chars() {
            // Type the character
            self.session
                .dispatch_key_event(KeyEventType::Char, None, Some(&ch.to_string()), None)
                .await?;

            // Variable delay based on character
            let base_delay = if ch == ' ' {
                random_range(min_delay + 30, max_delay + 30)
            } else if ch.is_ascii_punctuation() {
                random_range(min_delay + 50, max_delay + 50)
            } else {
                random_range(min_delay, max_delay)
            };

            // Occasional thinking pause
            let delay = if matches!(self.speed, HumanSpeed::Normal | HumanSpeed::Slow)
                && random_bool(0.05)
            {
                base_delay + random_range(200, 500)
            } else {
                base_delay
            };

            sleep(Duration::from_millis(delay)).await;

            // Occasional typo (slow mode only)
            if matches!(self.speed, HumanSpeed::Slow) && random_bool(0.01) && text.len() > 10 {
                let wrong_char = (b'a' + random_range(0, 26) as u8) as char;
                self.session
                    .dispatch_key_event(
                        KeyEventType::Char,
                        None,
                        Some(&wrong_char.to_string()),
                        None,
                    )
                    .await?;
                sleep(Duration::from_millis(random_range(100, 300))).await;

                // Backspace
                self.session
                    .dispatch_key_event(
                        KeyEventType::KeyDown,
                        Some("Backspace"),
                        None,
                        Some("Backspace"),
                    )
                    .await?;
                self.session
                    .dispatch_key_event(
                        KeyEventType::KeyUp,
                        Some("Backspace"),
                        None,
                        Some("Backspace"),
                    )
                    .await?;
                sleep(Duration::from_millis(random_range(50, 150))).await;
            }
        }

        Ok(())
    }

    /// Press a key
    pub async fn press_key(&self, key: &str) -> Result<()> {
        self.session
            .dispatch_key_event(KeyEventType::KeyDown, Some(key), None, Some(key))
            .await?;

        sleep(Duration::from_millis(random_range(50, 100))).await;

        self.session
            .dispatch_key_event(KeyEventType::KeyUp, Some(key), None, Some(key))
            .await?;

        Ok(())
    }

    /// Scroll the page by delta_y pixels (positive = down, negative = up)
    pub async fn scroll(&self, delta_y: f64) -> Result<()> {
        let num_scrolls = random_range(3, 8);
        let per_scroll = delta_y / num_scrolls as f64;

        for _ in 0..num_scrolls {
            let jitter = random_f64_range(-20.0, 20.0);
            let scroll_amount = per_scroll + jitter;

            self.session
                .dispatch_mouse_wheel(
                    random_f64_range(400.0, 800.0),
                    random_f64_range(300.0, 600.0),
                    0.0,
                    scroll_amount,
                )
                .await?;

            sleep(Duration::from_millis(random_range(30, 100))).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bezier_curve_endpoints() {
        let start = (50.0, 75.0);
        let end = (200.0, 300.0);

        let points = bezier_curve(start, end, 10);

        let first = points.first().unwrap();
        assert!((first.0 - start.0).abs() < 0.001);
        assert!((first.1 - start.1).abs() < 0.001);

        let last = points.last().unwrap();
        assert!((last.0 - end.0).abs() < 0.001);
        assert!((last.1 - end.1).abs() < 0.001);
    }

    #[test]
    fn test_human_speed_mouse_points() {
        let distance = 500.0;

        let fast = HumanSpeed::Fast.mouse_points(distance);
        let normal = HumanSpeed::Normal.mouse_points(distance);
        let slow = HumanSpeed::Slow.mouse_points(distance);

        assert!(fast < normal);
        assert!(normal < slow);
    }
}
