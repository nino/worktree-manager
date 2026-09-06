//! The buttons in rows. AppKit leaves the controls inside a table's cells out
//! of the window's key view loop, and buttons only take keyboard focus when
//! Full Keyboard Access is on. These buttons always take focus, and ask the
//! controller who comes before and after them (see `controller::key_view`),
//! so Tab walks every button in every row.
//!
//! [`PillButton`] adds a drawn bezel for the branch picker: a small rounded
//! control with a shaded, grained face, so it reads as something to click
//! rather than as the branch name in bold. [`IconButton`] is the glyph-only
//! kind, which explains itself through a quick tooltip of our own (see
//! `tooltip`) rather than the system's slow one.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{
    define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSAccessibility, NSBezierPath, NSButton, NSColor, NSEvent, NSGradient, NSImage, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

define_class!(
    #[unsafe(super(NSButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMButton"]
    pub struct Button;

    impl Button {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            self.isEnabled()
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            self.isEnabled() && !self.isHiddenOrHasHiddenAncestor()
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // Keyboard focus must be visible: bring the row into view.
            self.scrollRectToVisible(self.bounds());
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }

        #[unsafe(method_id(nextValidKeyView))]
        fn next_valid_key_view(&self) -> Option<Retained<NSView>> {
            crate::controller::key_view(self, true)
        }

        #[unsafe(method_id(previousValidKeyView))]
        fn previous_valid_key_view(&self) -> Option<Retained<NSView>> {
            crate::controller::key_view(self, false)
        }
    }
);

pub struct IconButtonIvars {
    hint: RefCell<String>,
}

define_class!(
    #[unsafe(super(Button))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMIconButton"]
    #[ivars = IconButtonIvars]
    pub struct IconButton;

    impl IconButton {
        /// Tracks the whole button, and follows it as rows scroll.
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    MainThreadMarker::from(self).alloc(),
                    NSRect::ZERO,
                    NSTrackingAreaOptions::MouseEnteredAndExited
                        | NSTrackingAreaOptions::ActiveInActiveApp
                        | NSTrackingAreaOptions::InVisibleRect,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _e: &NSEvent) {
            crate::tooltip::schedule(self, &self.ivars().hint.borrow());
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _e: &NSEvent) {
            crate::tooltip::hide();
        }

        /// A tooltip that stayed up over a button being clicked would sit on
        /// top of whatever the click opens.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            crate::tooltip::hide();
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
        }
    }
);

impl IconButton {
    pub fn new(image: &NSImage, hint: &str, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(IconButtonIvars {
            hint: RefCell::new(String::new()),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(20.0, 20.0))]
        };
        this.setImage(Some(image));
        this.set_hint(hint);
        this
    }

    /// The tooltip text, which is also the button's accessibility help — the
    /// system tooltip used to provide that, and these buttons no longer set
    /// one. (Their name comes from the symbol's accessibility description.)
    pub fn set_hint(&self, hint: &str) {
        *self.ivars().hint.borrow_mut() = hint.to_string();
        self.setAccessibilityHelp(Some(&crate::util::ns(hint)));
    }
}

define_class!(
    #[unsafe(super(Button))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMPillButton"]
    pub struct PillButton;

    impl PillButton {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            draw_pill(self.bounds(), self.isEnabled(), self.isHighlighted());
            let _: () = unsafe { msg_send![super(self), drawRect: dirty] };
        }

        /// The focus ring follows the bezel; without this AppKit rings the
        /// chevrons alone, which is all the button's image cell knows about.
        #[unsafe(method(drawFocusRingMask))]
        fn draw_focus_ring_mask(&self) {
            pill_path(self.bounds()).fill();
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn focus_ring_mask_bounds(&self) -> NSRect {
            self.bounds()
        }

        /// Room for the bezel around the title.
        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            let s: NSSize = unsafe { msg_send![super(self), intrinsicContentSize] };
            NSSize::new(s.width + 2.0 * PILL_PAD_X, s.height + 2.0 * PILL_PAD_Y)
        }
    }
);

/// Padding the bezel adds around the title and chevrons.
const PILL_PAD_X: f64 = 7.0;
const PILL_PAD_Y: f64 = 1.5;
const PILL_RADIUS: f64 = 5.0;

fn pill_path(bounds: NSRect) -> Retained<NSBezierPath> {
    let rect = NSRect::new(
        NSPoint::new(0.5, 0.5),
        NSSize::new(bounds.size.width - 1.0, bounds.size.height - 1.0),
    );
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, PILL_RADIUS, PILL_RADIUS)
}

/// The bezel: a face shaded from dark at the top down to the plate's own
/// colour, grained like the repo header band, with a bevel hairline and a
/// border; darker while pressed. Buttons are unflipped, so y grows upwards.
fn draw_pill(bounds: NSRect, enabled: bool, pressed: bool) {
    let alpha = if enabled { 1.0 } else { 0.45 };
    let path = pill_path(bounds);
    let rect = path.bounds();
    let base = NSColor::controlBackgroundColor();
    let shade = |amount: f64| {
        base.blendedColorWithFraction_ofColor(amount, &NSColor::blackColor())
            .unwrap_or_else(|| base.retain())
            .colorWithAlphaComponent(alpha)
    };
    let (top, bottom) = if pressed {
        (shade(0.20), shade(0.12))
    } else {
        (shade(0.09), shade(0.0))
    };
    let mtm = MainThreadMarker::new().expect("drawing happens on the main thread");
    if let Some(g) = NSGradient::initWithStartingColor_endingColor(mtm.alloc(), &bottom, &top) {
        g.drawInBezierPath_angle(&path, 90.0);
    }
    crate::rowview::grain().setFill();
    path.fill();
    // A bright hairline under the top edge, the same bevel the plates have.
    let bevel = NSRect::new(
        NSPoint::new(rect.origin.x + PILL_RADIUS, rect.size.height - 1.0),
        NSSize::new(rect.size.width - 2.0 * PILL_RADIUS, 1.0),
    );
    NSColor::whiteColor()
        .colorWithAlphaComponent(0.45 * alpha)
        .setFill();
    NSBezierPath::bezierPathWithRect(bevel).fill();
    NSColor::separatorColor()
        .colorWithAlphaComponent(0.32 * alpha)
        .setStroke();
    path.setLineWidth(1.0);
    path.stroke();
}

impl PillButton {
    pub fn with_title(
        title: &NSString,
        target: Option<&AnyObject>,
        action: Sel,
        _mtm: MainThreadMarker,
    ) -> Retained<Self> {
        unsafe {
            msg_send![
                Self::class(),
                buttonWithTitle: title,
                target: target,
                action: Some(action)
            ]
        }
    }
}

impl Button {
    pub fn with_image(image: &NSImage, _mtm: MainThreadMarker) -> Retained<Self> {
        unsafe {
            msg_send![
                Self::class(),
                buttonWithImage: image,
                target: None::<&AnyObject>,
                action: None::<Sel>
            ]
        }
    }

    pub fn with_title(
        title: &NSString,
        target: Option<&AnyObject>,
        action: Sel,
        _mtm: MainThreadMarker,
    ) -> Retained<Self> {
        unsafe {
            msg_send![
                Self::class(),
                buttonWithTitle: title,
                target: target,
                action: Some(action)
            ]
        }
    }
}
