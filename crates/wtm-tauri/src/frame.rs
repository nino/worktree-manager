//! The window's frame, kept in `ui-state.json` the way the native build
//! keeps it, so that switching between the two builds leaves the window
//! where it was.
//!
//! tao measures from the top-left of the main screen downwards; AppKit, and
//! so the native macOS build's saved frame, from the bottom-left upwards.
//! On macOS a frame is flipped on the way in and out; elsewhere both builds
//! measure from the top.

use wtm_core::WindowFrame;

/// A rectangle in logical points, measured from the top-left of the main
/// screen (tao's coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// How saved frames are measured on this OS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origin {
    /// From the top-left, as tao does.
    Top,
    /// From the bottom-left of a main screen this tall (AppKit).
    Bottom { main_height: f64 },
}

impl Origin {
    pub fn save(self, r: Rect) -> WindowFrame {
        WindowFrame {
            x: r.x,
            y: self.flip(r.y, r.height),
            width: r.width,
            height: r.height,
        }
    }

    pub fn load(self, f: WindowFrame) -> Rect {
        Rect {
            x: f.x,
            y: self.flip(f.y, f.height),
            width: f.width,
            height: f.height,
        }
    }

    /// The same flip both ways: a top edge to a bottom edge and back.
    fn flip(self, y: f64, height: f64) -> f64 {
        match self {
            Origin::Top => y,
            Origin::Bottom { main_height } => main_height - y - height,
        }
    }
}

/// Where the window should open: the saved frame, unless too little of it
/// would land on the screens there are now (`WindowFrame::is_usable_on`);
/// then `None`, and the window centres.
pub fn restore(saved: Option<WindowFrame>, origin: Origin, screens: &[Rect]) -> Option<Rect> {
    let saved = saved?;
    let screens: Vec<WindowFrame> = screens.iter().map(|s| origin.save(*s)).collect();
    // With no screens reported there is nothing to check against.
    (screens.is_empty() || saved.is_usable_on(&screens)).then(|| origin.load(saved))
}

/// A first launch's size, shrunk to fit `screen` (the main screen's work
/// area) so that centring it never puts the toolbar off the top.
pub fn first_size(screen: Option<Rect>) -> (f64, f64) {
    const WIDTH: f64 = 940.0;
    const HEIGHT: f64 = 760.0;
    match screen {
        Some(s) => (WIDTH.min(s.width), HEIGHT.min(s.height)),
        None => (WIDTH, HEIGHT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    };

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn appkit_frames_are_flipped_both_ways() {
        let o = Origin::Bottom { main_height: 900.0 };
        let r = rect(100.0, 50.0, 800.0, 600.0);
        let f = o.save(r);
        // 50 from the top with 600 of height leaves 250 below.
        assert_eq!((f.x, f.y, f.width, f.height), (100.0, 250.0, 800.0, 600.0));
        assert_eq!(o.load(f), r);
    }

    #[test]
    fn top_origin_frames_are_kept_as_they_are() {
        let r = rect(10.0, 20.0, 300.0, 400.0);
        assert_eq!(Origin::Top.load(Origin::Top.save(r)), r);
    }

    #[test]
    fn a_frame_on_a_screen_comes_back() {
        let o = Origin::Bottom { main_height: 900.0 };
        let r = rect(100.0, 50.0, 800.0, 600.0);
        assert_eq!(restore(Some(o.save(r)), o, &[MAIN]), Some(r));
    }

    #[test]
    fn a_frame_on_an_unplugged_screen_is_dropped() {
        // Saved on a second screen to the right, which is gone.
        let o = Origin::Top;
        let r = rect(1600.0, 100.0, 800.0, 600.0);
        assert_eq!(restore(Some(o.save(r)), o, &[MAIN]), None);
        let second = rect(1440.0, 0.0, 1920.0, 1080.0);
        assert_eq!(restore(Some(o.save(r)), o, &[MAIN, second]), Some(r));
    }

    #[test]
    fn nothing_saved_centres() {
        assert_eq!(restore(None, Origin::Top, &[MAIN]), None);
    }

    #[test]
    fn a_first_window_fits_a_small_screen() {
        assert_eq!(first_size(Some(MAIN)), (940.0, 760.0));
        assert_eq!(
            first_size(Some(rect(0.0, 25.0, 1280.0, 695.0))),
            (940.0, 695.0)
        );
        assert_eq!(first_size(None), (940.0, 760.0));
    }
}
