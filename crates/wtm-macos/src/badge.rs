//! A small rounded status badge ("staged", "↑3 origin/main"), drawn natively:
//! a tinted capsule behind a system-font label. In the light appearance each
//! state has its own hue; in the dark one colour is reserved for uncommitted
//! work and a missing folder (`Emphasis::Attention`/`Alarm`), and everything
//! else is luminance. Which badges a row has is `wtm_ui::list::badges_for`.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAppearanceCustomization, NSBezierPath, NSColor, NSFont, NSLayoutConstraint, NSTextField,
    NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};
use std::cell::RefCell;

use wtm_toolkit::{Emphasis, Hue};

use crate::util::{drawing_dark, ns};

/// A badge's colour family and how loud it is: `wtm_ui` decides both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadgeTone {
    pub hue: Hue,
    pub emphasis: Emphasis,
}

/// How loud a badge is drawn when dark. Only Attention and Alarm spend colour.
type BadgeRank = Emphasis;

impl BadgeTone {
    /// The light appearance's colour: one system hue per state. Against
    /// near-black a stack of rows turns these into a repeating high-chroma
    /// pattern, which is why dark draws by [`BadgeRank`] instead.
    fn hue(self) -> Retained<NSColor> {
        match self.hue {
            Hue::Green => NSColor::systemGreenColor(),
            Hue::Blue => NSColor::systemBlueColor(),
            Hue::Orange => NSColor::systemOrangeColor(),
            Hue::Purple => NSColor::systemPurpleColor(),
            Hue::Teal => NSColor::systemTealColor(),
            Hue::Red => NSColor::systemRedColor(),
            Hue::Yellow => NSColor::systemYellowColor(),
            Hue::Gray => NSColor::systemGrayColor(),
            Hue::Accent => NSColor::controlAccentColor(),
        }
    }

    /// Label colour under the current drawing appearance.
    fn ink(self) -> Retained<NSColor> {
        if drawing_dark() {
            rank_ink(self.emphasis)
        } else {
            self.hue()
        }
    }

    /// Capsule colour under the current drawing appearance.
    fn fill(self) -> Retained<NSColor> {
        if drawing_dark() {
            rank_fill(self.emphasis)
        } else {
            self.hue().colorWithAlphaComponent(0.16)
        }
    }
}

fn rank_ink(rank: BadgeRank) -> Retained<NSColor> {
    match rank {
        BadgeRank::Quiet => NSColor::secondaryLabelColor(),
        BadgeRank::Notable => NSColor::labelColor(),
        BadgeRank::Attention => muted(&NSColor::systemOrangeColor()),
        BadgeRank::Alarm => muted(&NSColor::systemRedColor()),
    }
}

fn rank_fill(rank: BadgeRank) -> Retained<NSColor> {
    match rank {
        BadgeRank::Quiet => NSColor::labelColor().colorWithAlphaComponent(0.06),
        BadgeRank::Notable => NSColor::labelColor().colorWithAlphaComponent(0.10),
        BadgeRank::Attention | BadgeRank::Alarm => rank_ink(rank).colorWithAlphaComponent(0.14),
    }
}

/// Blend a system hue toward the foreground so it pastels on dark. Resolves
/// now; rebuild under the view's appearance if that changes.
fn muted(base: &NSColor) -> Retained<NSColor> {
    base.blendedColorWithFraction_ofColor(0.3, &NSColor::labelColor())
        .unwrap_or_else(|| base.retain())
}

pub struct BadgeIvars {
    label: Retained<NSTextField>,
    tone: RefCell<BadgeTone>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMBadge"]
    #[ivars = BadgeIvars]
    pub struct Badge;

    impl Badge {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let r = bounds.size.height / 2.0;
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, r, r);
            self.ivars().tone.borrow().fill().setFill();
            path.fill();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn view_did_change_effective_appearance(&self) {
            self.paint_ink();
        }
    }
);

impl Badge {
    pub fn new(
        text: &str,
        tone: BadgeTone,
        tooltip: &str,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let label = NSTextField::labelWithString(&ns(text), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize_weight(
            10.5,
            crate::util::MEDIUM,
        )));
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        let this = mtm.alloc::<Self>().set_ivars(BadgeIvars {
            label: label.clone(),
            tone: RefCell::new(tone),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(40.0, 16.0))]
        };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&label);
        this.setToolTip(Some(&ns(tooltip)));
        let constraints = [
            label
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&this.leadingAnchor(), 6.0),
            label
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&this.trailingAnchor(), -6.0),
            label
                .topAnchor()
                .constraintEqualToAnchor_constant(&this.topAnchor(), 1.5),
            label
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&this.bottomAnchor(), -1.5),
        ];
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        this.paint_ink();
        this
    }

    /// Re-style in place (cheaper than replacing the view).
    pub fn set(&self, text: &str, tone: BadgeTone, tooltip: &str) {
        let iv = self.ivars();
        iv.label.setStringValue(&ns(text));
        *iv.tone.borrow_mut() = tone;
        self.setToolTip(Some(&ns(tooltip)));
        self.paint_ink();
    }

    /// Label ink is set here, not in `drawRect:`, under this view's appearance:
    /// that picks the scheme, and blends snapshot at resolve.
    fn paint_ink(&self) {
        let tone = *self.ivars().tone.borrow();
        self.setNeedsDisplay(true);
        let label = self.ivars().label.clone();
        self.effectiveAppearance()
            .performAsCurrentDrawingAppearance(&RcBlock::new(move || {
                label.setTextColor(Some(&tone.ink()));
            }));
    }
}
