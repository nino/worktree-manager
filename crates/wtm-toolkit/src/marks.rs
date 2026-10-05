//! The agent marks, as shapes on a 16-unit grid with y growing downwards —
//! the same grid as the web app's SVGs — for toolkits that draw them from
//! data. Clawd carries his own colours; Cursor's cube is drawn in the colour
//! of the text around it (`Paint::Ink`), so it follows a selected row's
//! text. The AppKit backend draws the same shapes in `toolicon.rs`.

use crate::view::Mark;

/// A colour as sRGB components in 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    Fixed(Rgb),
    /// The surrounding text's colour at this opacity.
    Ink(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    RoundedRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        paint: Paint,
    },
    Polygon {
        points: Vec<(f64, f64)>,
        paint: Paint,
    },
    /// Separate straight strokes.
    Lines {
        segments: Vec<((f64, f64), (f64, f64))>,
        width: f64,
        paint: Paint,
    },
}

/// Anthropic terracotta (Clawd's body) and the dark of his eyes.
const CLAWD_BODY: Rgb = Rgb(0.851, 0.467, 0.341);
const CLAWD_INK: Rgb = Rgb(0.090, 0.094, 0.102);

/// Size of a mark next to 12-point text, and the space after it.
pub const MARK_SIZE: f64 = 13.0;
pub const MARK_GAP: f64 = 3.0;

pub fn shapes(mark: Mark) -> Vec<Shape> {
    let rect = |x, y, w, h, r, paint| Shape::RoundedRect {
        x,
        y,
        w,
        h,
        r,
        paint,
    };
    match mark {
        Mark::Claude => vec![
            rect(2.0, 4.5, 12.0, 7.5, 2.5, Paint::Fixed(CLAWD_BODY)),
            rect(4.0, 12.5, 2.4, 2.0, 0.5, Paint::Fixed(CLAWD_BODY)),
            rect(9.6, 12.5, 2.4, 2.0, 0.5, Paint::Fixed(CLAWD_BODY)),
            rect(4.5, 5.0, 2.2, 3.6, 0.6, Paint::Fixed(CLAWD_INK)),
            rect(9.3, 5.0, 2.2, 3.6, 0.6, Paint::Fixed(CLAWD_INK)),
        ],
        Mark::Cursor => vec![
            Shape::Polygon {
                points: vec![(8.0, 1.5), (14.0, 5.0), (8.0, 8.5), (2.0, 5.0)],
                paint: Paint::Ink(0.9),
            },
            Shape::Polygon {
                points: vec![(2.0, 5.0), (2.0, 11.5), (8.0, 15.0), (8.0, 8.5)],
                paint: Paint::Ink(0.55),
            },
            Shape::Polygon {
                points: vec![(14.0, 5.0), (14.0, 11.5), (8.0, 15.0), (8.0, 8.5)],
                paint: Paint::Ink(0.35),
            },
            // The hidden-edge fold that makes it read as Cursor's mark.
            Shape::Lines {
                segments: vec![((2.0, 5.0), (14.0, 11.5)), ((8.0, 8.5), (8.0, 15.0))],
                width: 0.9,
                paint: Paint::Ink(0.6),
            },
        ],
    }
}
