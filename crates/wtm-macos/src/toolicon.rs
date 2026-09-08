//! The marks that stand in for a `claude/` or `cursor/` branch prefix, drawn
//! on the same 16-unit grid as the web app's SVGs so the two apps look alike.
//! Clawd carries his own colours; Cursor's cube is drawn in the colour of the
//! text it sits in, which is passed in — a template image would not follow
//! the text when a picker row is selected.

use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSBezierPath, NSColor, NSImage};
use objc2_foundation::NSCopying;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use wtm_core::branch_tool::BranchTool;

/// Anthropic terracotta (Clawd's body) and the dark of his eyes.
fn clawd_body() -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(0.851, 0.467, 0.341, 1.0)
}

fn clawd_ink() -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(0.090, 0.094, 0.102, 1.0)
}

/// The mark for `tool`: `size` points square, with `gap` points of empty
/// space after it so the branch name does not touch it. `ink` colours the
/// Cursor cube; Clawd ignores it.
pub fn mark(tool: BranchTool, size: f64, gap: f64, ink: &NSColor) -> Retained<NSImage> {
    let ink = ink.copy();
    let handler = block2::RcBlock::new(move |_rect: NSRect| -> Bool {
        let u = size / 16.0;
        match tool {
            BranchTool::Claude => draw_clawd(u),
            BranchTool::Cursor => draw_cursor(u, &ink),
        }
        Bool::YES
    });
    // Flipped, so the grid matches the SVG's (y downwards).
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(size + gap, size), true, &handler)
}

fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64, u: f64) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
        NSRect::new(NSPoint::new(x * u, y * u), NSSize::new(w * u, h * u)),
        r * u,
        r * u,
    )
}

/// Clawd, the Claude Code mascot: a block with two eyes and stubby legs.
fn draw_clawd(u: f64) {
    clawd_body().setFill();
    rounded(2.0, 4.5, 12.0, 7.5, 2.5, u).fill();
    rounded(4.0, 12.5, 2.4, 2.0, 0.5, u).fill();
    rounded(9.6, 12.5, 2.4, 2.0, 0.5, u).fill();
    clawd_ink().setFill();
    rounded(4.5, 5.0, 2.2, 3.6, 0.6, u).fill();
    rounded(9.3, 5.0, 2.2, 3.6, 0.6, u).fill();
}

/// Cursor's isometric cube, its three faces at different weights.
fn draw_cursor(u: f64, ink: &NSColor) {
    let face = |points: &[(f64, f64)], alpha: f64| {
        let path = NSBezierPath::new();
        for (i, (x, y)) in points.iter().enumerate() {
            let p = NSPoint::new(x * u, y * u);
            if i == 0 {
                path.moveToPoint(p);
            } else {
                path.lineToPoint(p);
            }
        }
        path.closePath();
        ink.colorWithAlphaComponent(alpha).setFill();
        path.fill();
    };
    face(&[(8.0, 1.5), (14.0, 5.0), (8.0, 8.5), (2.0, 5.0)], 0.9);
    face(&[(2.0, 5.0), (2.0, 11.5), (8.0, 15.0), (8.0, 8.5)], 0.55);
    face(&[(14.0, 5.0), (14.0, 11.5), (8.0, 15.0), (8.0, 8.5)], 0.35);
    // The hidden-edge fold that makes it read as Cursor's mark, not a cube.
    let fold = NSBezierPath::new();
    fold.moveToPoint(NSPoint::new(2.0 * u, 5.0 * u));
    fold.lineToPoint(NSPoint::new(14.0 * u, 11.5 * u));
    fold.moveToPoint(NSPoint::new(8.0 * u, 8.5 * u));
    fold.lineToPoint(NSPoint::new(8.0 * u, 15.0 * u));
    fold.setLineWidth(0.9 * u);
    ink.colorWithAlphaComponent(0.6).setStroke();
    fold.stroke();
}
