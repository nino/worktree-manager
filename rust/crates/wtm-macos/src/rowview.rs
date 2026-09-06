//! Row backgrounds that give the flat outline a visible hierarchy: every repo
//! is a raised card (header with a soft gradient and a top highlight, straight
//! sides down its worktrees, rounded and shadowed bottom), and every worktree
//! sits on its own inset plate inside the card. Each row draws its own slice
//! of the card, so the outline's virtualisation and row reuse are untouched.
//!
//! Colours are all system colours or blends of them, so light and dark
//! appearance both work; the gradient and highlight are the nod to Aqua.

use std::cell::{Cell, RefCell};

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSBezierPath, NSBitmapImageRep, NSColor, NSGradient, NSGraphicsContext, NSShadow,
    NSTableRowView, NSView, NSViewLayerContentsRedrawPolicy,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

/// Space above each card (between cards).
pub const CARD_GAP: f64 = 12.0;
/// Horizontal margin of the cards inside the outline.
pub const CARD_MARGIN: f64 = 14.0;
const CARD_RADIUS: f64 = 10.0;
/// Horizontal inset of a worktree plate inside its card.
pub const PLATE_INSET: f64 = 10.0;
/// Padding at the top and bottom of the well (band → first plate, last
/// plate → card bottom). The first child row carries the top part above its
/// plate, the last child row the bottom part below its plate.
pub const WELL_PAD: f64 = 10.0;
/// The well's lead-in below the header band, carried by the first child row
/// so the header's height never changes when it opens or closes.
pub const WELL_LEAD: f64 = WELL_PAD - PLATE_GAP;
/// Vertical gap around a worktree plate (between plates and to the card edges).
pub const PLATE_GAP: f64 = 4.0;
const PLATE_RADIUS: f64 = 7.0;
/// Room left under a card's bottom edge for its shadow.
pub const CARD_BOTTOM_ROOM: f64 = 4.0;
/// Extra height of a card's last row: padding between the last plate and the
/// card's bottom edge, so it matches the padding between plates.
pub const LAST_ROW_EXTRA: f64 = WELL_PAD - PLATE_GAP + CARD_BOTTOM_ROOM;

/// Which slice of a card a row draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStyle {
    /// Repo header. `closed`: no rows follow (collapsed or empty), so the
    /// card's bottom edge is drawn here too.
    Header { closed: bool },
    /// A worktree/pending row. `first`: carries the well's lead-in under the
    /// band; `last`: closes the card underneath the plate.
    Child { first: bool, last: bool },
}

pub struct RowViewIvars {
    style: Cell<RowStyle>,
    /// Repo id of the card this row belongs to.
    owner: RefCell<String>,
    /// A rendering of the whole row, shown in place of its subviews while
    /// the outline slides the row away (see `set_snapshot`).
    snapshot: RefCell<Option<Retained<NSBitmapImageRep>>>,
}

define_class!(
    #[unsafe(super(NSTableRowView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMRowView"]
    #[ivars = RowViewIvars]
    pub struct RowView;

    impl RowView {
        #[unsafe(method(drawBackgroundInRect:))]
        fn draw_background(&self, _dirty: NSRect) {
            draw(self.bounds(), self.ivars().style.get());
        }

        #[unsafe(method(drawSelectionInRect:))]
        fn draw_selection(&self, _dirty: NSRect) {}

        /// While a snapshot is set the row is a plain image layer (see
        /// `set_snapshot`).
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            self.ivars().snapshot.borrow().is_some()
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.apply_snapshot();
        }

        #[unsafe(method(drawSeparatorInRect:))]
        fn draw_separator(&self, _dirty: NSRect) {}

        /// A collapse moves the card's rows out of the outline into a clip
        /// view, slides them away, and drops them from there. That last
        /// removal is when the header can close. A row leaving the outline
        /// directly is either about to enter that clip view (the animated
        /// case) or gone for good (no animation, or recycled): a short
        /// delay tells the two apart.
        #[unsafe(method(viewWillMoveToSuperview:))]
        fn view_will_move_to_superview(&self, superview: Option<&NSView>) {
            if superview.is_some() || !matches!(self.ivars().style.get(), RowStyle::Child { .. }) {
                return;
            }
            let in_outline = unsafe { self.superview() }
                .map(|sv| sv.isKindOfClass(objc2_app_kit::NSTableView::class()))
                .unwrap_or(true);
            if !in_outline {
                crate::controller::child_row_leaving(&self.ivars().owner.borrow());
                return;
            }
            let mtm = MainThreadMarker::from(self);
            let me = MainThreadBound::new(self.retain(), mtm);
            let _ = DispatchQueue::main().after(
                DispatchTime::NOW.time(50_000_000),
                move || {
                    let mtm = MainThreadMarker::new().expect("main queue");
                    let me = me.get(mtm);
                    if unsafe { me.superview() }.is_none() {
                        crate::controller::child_row_leaving(&me.ivars().owner.borrow());
                    }
                },
            );
        }
    }
);

impl RowView {
    pub const IDENTIFIER: &'static str = "wtm.row";

    pub fn new(style: RowStyle, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(RowViewIvars {
            style: Cell::new(style),
            owner: RefCell::new(String::new()),
            snapshot: RefCell::new(None),
        });
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(400.0, 40.0));
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this
    }

    /// Collapsing a card, the outline moves its rows into a clip view and
    /// slides them away — but never lets anything draw in there, so text and
    /// card slices would vanish for the animation and only image layers
    /// survive. So before the move each row is rendered to a bitmap that
    /// becomes the row layer's contents, and its subviews are hidden. `None`
    /// restores the live row (the outline reuses row views).
    pub fn set_snapshot(&self, rep: Option<Retained<NSBitmapImageRep>>) {
        let has = rep.is_some();
        let had = self.ivars().snapshot.borrow().is_some();
        if !has && !had {
            return;
        }
        *self.ivars().snapshot.borrow_mut() = rep;
        for sv in self.subviews().iter() {
            sv.setHidden(has);
        }
        self.setLayerContentsRedrawPolicy(if has {
            NSViewLayerContentsRedrawPolicy::Never
        } else {
            NSViewLayerContentsRedrawPolicy::DuringViewResize
        });
        self.apply_snapshot();
        self.setNeedsDisplay(true);
    }

    fn apply_snapshot(&self) {
        let Some(layer) = self.layer() else { return };
        let snapshot = self.ivars().snapshot.borrow();
        unsafe {
            let image: *mut AnyObject = match snapshot.as_ref() {
                Some(rep) => msg_send![&**rep, CGImage],
                None => std::ptr::null_mut(),
            };
            let _: () = msg_send![&*layer, setContents: image];
        }
    }

    pub fn set_owner(&self, repo_id: &str) {
        *self.ivars().owner.borrow_mut() = repo_id.to_string();
    }

    pub fn owner(&self) -> String {
        self.ivars().owner.borrow().clone()
    }

    pub fn style(&self) -> RowStyle {
        self.ivars().style.get()
    }

    /// Returns true when the style changed.
    pub fn set_style(&self, style: RowStyle) -> bool {
        if self.ivars().style.get() == style {
            return false;
        }
        self.ivars().style.set(style);
        self.setNeedsDisplay(true);
        true
    }
}

// MARK: Palette

fn card_fill() -> Retained<NSColor> {
    NSColor::controlBackgroundColor()
}

/// Strokes are the separator colour, thinned: they should outline, not draw
/// attention.
fn card_border() -> Retained<NSColor> {
    NSColor::separatorColor().colorWithAlphaComponent(0.55)
}

/// The well the plates sit in: the window ground, toned a little towards the
/// card, so it reads as recessed under the raised plates.
fn well_fill() -> Retained<NSColor> {
    NSColor::windowBackgroundColor()
        .blendedColorWithFraction_ofColor(0.02, &NSColor::blackColor())
        .unwrap_or_else(NSColor::windowBackgroundColor)
}

/// The grain tile: a few thousand half-point specks of black and white at
/// low alpha, baked once and tiled by Core Graphics as a pattern colour.
/// Cheap to draw and appearance-neutral.
fn grain() -> Retained<NSColor> {
    thread_local! {
        static GRAIN: std::cell::OnceCell<Retained<NSColor>> = const { std::cell::OnceCell::new() };
    }
    GRAIN.with(|g| {
        g.get_or_init(|| {
            let size = NSSize::new(128.0, 128.0);
            let handler = block2::RcBlock::new(move |_rect: NSRect| -> objc2::runtime::Bool {
                // xorshift: deterministic, so every tile edge matches.
                let mut state: u32 = 0x9E37_79B9;
                let mut next = || {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state
                };
                for _ in 0..2600 {
                    let x = (next() % 256) as f64 * 0.5;
                    let y = (next() % 256) as f64 * 0.5;
                    let light = next() % 2 == 0;
                    let alpha = 0.04 + (next() % 5) as f64 * 0.012;
                    let c = if light {
                        NSColor::whiteColor()
                    } else {
                        NSColor::blackColor()
                    };
                    c.colorWithAlphaComponent(alpha).setFill();
                    NSBezierPath::bezierPathWithRect(NSRect::new(
                        NSPoint::new(x, y),
                        NSSize::new(0.5, 0.5),
                    ))
                    .fill();
                }
                objc2::runtime::Bool::YES
            });
            let image =
                objc2_app_kit::NSImage::imageWithSize_flipped_drawingHandler(size, false, &handler);
            NSColor::colorWithPatternImage(&image)
        })
        .clone()
    })
}

/// Plates are raised: the card colour, lifted by a drop shadow.
fn plate_fill() -> Retained<NSColor> {
    NSColor::controlBackgroundColor()
}

/// The well's side shading, drawn by every row the well passes through so
/// the strips are continuous from the band down to the card bottom.
fn well_sides(well: NSRect) {
    let side = 5.0;
    inner_shadow(
        NSRect::new(well.origin, NSSize::new(side, well.size.height)),
        0.0,
        0.06,
    );
    inner_shadow(
        NSRect::new(
            NSPoint::new(well.origin.x + well.size.width - side, well.origin.y),
            NSSize::new(side, well.size.height),
        ),
        180.0,
        0.06,
    );
}

/// Inner shadow along one edge of the well: a short gradient from shade to
/// clear. `angle` is NSGradient's, in unflipped coordinates: rows are flipped,
/// so 90 fades downwards on screen and −90 upwards; 0 and 180 are unaffected.
fn inner_shadow(rect: NSRect, angle: f64, strength: f64) {
    inner_shadow_to(rect, angle, strength, 0.0);
}

fn inner_shadow_to(rect: NSRect, angle: f64, from_alpha: f64, to_alpha: f64) {
    let from = NSColor::blackColor().colorWithAlphaComponent(from_alpha);
    let to = NSColor::blackColor().colorWithAlphaComponent(to_alpha);
    let mtm = MainThreadMarker::new().expect("drawing happens on the main thread");
    if let Some(g) = NSGradient::initWithStartingColor_endingColor(mtm.alloc(), &from, &to) {
        g.drawInRect_angle(rect, angle);
    }
}

fn shadow(blur: f64, dy: f64, alpha: f64) {
    let s = NSShadow::new();
    s.setShadowColor(Some(&NSColor::blackColor().colorWithAlphaComponent(alpha)));
    s.setShadowBlurRadius(blur);
    s.setShadowOffset(NSSize::new(0.0, -dy));
    s.set();
}

// MARK: Drawing

/// Rows are flipped (y grows downward), as `NSTableRowView` is.
fn draw(bounds: NSRect, style: RowStyle) {
    let w = bounds.size.width;
    let h = bounds.size.height;
    let x = CARD_MARGIN;
    let cw = w - 2.0 * CARD_MARGIN;
    // Half-pixel alignment keeps 1px strokes crisp.
    let r = CARD_RADIUS;
    match style {
        RowStyle::Header { closed } => {
            // The card starts CARD_GAP below the row top; when open it extends
            // past the row bottom (clipped) so only the top corners round.
            let extra = if closed { 0.0 } else { r + 2.0 };
            let rect = NSRect::new(
                NSPoint::new(x + 0.5, CARD_GAP + 0.5),
                NSSize::new(cw - 1.0, h - CARD_GAP - 1.0 + extra),
            );
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, r, r);
            if closed {
                NSGraphicsContext::saveGraphicsState_class();
                shadow(3.0, 1.0, 0.14);
                card_fill().setFill();
                path.fill();
                NSGraphicsContext::restoreGraphicsState_class();
            } else {
                card_fill().setFill();
                path.fill();
            }
            // Aqua-style header: a soft vertical gradient over the header band
            // plus a bright hairline along the top edge. The band fills the row
            // to its bottom edge; when the card is open its path runs past the
            // row bottom so only its top corners are rounded (the rest is
            // clipped), and the first child row carries the well's lead-in.
            let band = NSRect::new(
                NSPoint::new(x + 1.0, CARD_GAP + 1.0),
                NSSize::new(cw - 2.0, h - CARD_GAP - 1.0 + extra),
            );
            let band_path =
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(band, r - 1.0, r - 1.0);
            // Aqua's blue, greyed down: the card colour tinted towards a
            // desaturated system blue and darkened a little, deeper at the
            // top and fading towards the plates.
            let base = card_fill();
            let blue = NSColor::systemBlueColor()
                .blendedColorWithFraction_ofColor(0.45, &NSColor::systemGrayColor())
                .unwrap_or_else(NSColor::systemBlueColor);
            let tone = |blue_amount: f64, dark_amount: f64| {
                base.blendedColorWithFraction_ofColor(blue_amount, &blue)
                    .and_then(|c| {
                        c.blendedColorWithFraction_ofColor(dark_amount, &NSColor::blackColor())
                    })
                    .unwrap_or_else(|| base.clone())
            };
            let top = tone(0.26, 0.06);
            let bottom = tone(0.12, 0.02);
            // A bright hairline along the top edge, the Aqua bevel.
            let hairline = NSRect::new(
                NSPoint::new(x + r, CARD_GAP + 1.0),
                NSSize::new(cw - 2.0 * r, 1.0),
            );
            if let Some(g) = NSGradient::initWithStartingColor_endingColor(
                MainThreadMarker::new()
                    .expect("drawing happens on the main thread")
                    .alloc(),
                &top,
                &bottom,
            ) {
                g.drawInBezierPath_angle(&band_path, -90.0);
            }
            // Grain over the band, like brushed metal under glass.
            grain().setFill();
            band_path.fill();
            NSColor::whiteColor()
                .colorWithAlphaComponent(0.35)
                .setFill();
            NSBezierPath::bezierPathWithRect(hairline).fill();
            card_border().setStroke();
            path.setLineWidth(1.0);
            path.stroke();
        }
        RowStyle::Child { first, last } => {
            // The first row is WELL_LEAD taller: the well's lead-in below the
            // band, before its plate.
            let lead = if first { WELL_LEAD } else { 0.0 };
            // Card body: a rounded rect taller than the row so the sides run
            // straight; for the last row its bottom corners are in view.
            let top = -(r + 2.0);
            let bottom = if last {
                h - CARD_BOTTOM_ROOM - 0.5
            } else {
                h + r + 2.0
            };
            let rect = NSRect::new(
                NSPoint::new(x + 0.5, top),
                NSSize::new(cw - 1.0, bottom - top),
            );
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, r, r);
            if last {
                NSGraphicsContext::saveGraphicsState_class();
                shadow(3.0, 1.0, 0.14);
                card_fill().setFill();
                path.fill();
                NSGraphicsContext::restoreGraphicsState_class();
            } else {
                card_fill().setFill();
                path.fill();
            }
            // The well: recessed ground with shading down both sides, clipped
            // to the card so its bottom corners follow the card's radius.
            let well_bottom = if last { h - CARD_BOTTOM_ROOM - 1.0 } else { h };
            let well = NSRect::new(
                NSPoint::new(x + 1.0, 0.0),
                NSSize::new(cw - 2.0, well_bottom),
            );
            NSGraphicsContext::saveGraphicsState_class();
            path.addClip();
            well_fill().setFill();
            NSBezierPath::bezierPathWithRect(well).fill();
            well_sides(well);
            if first {
                // Separator between the band and the well, then the well's
                // inner shadow fading downwards from it.
                NSColor::separatorColor()
                    .colorWithAlphaComponent(0.4)
                    .setFill();
                NSBezierPath::bezierPathWithRect(NSRect::new(
                    well.origin,
                    NSSize::new(well.size.width, 1.0),
                ))
                .fill();
                inner_shadow(
                    NSRect::new(
                        NSPoint::new(x + 1.0, 1.0),
                        NSSize::new(cw - 2.0, lead + 3.0),
                    ),
                    90.0,
                    0.10,
                );
            }
            if last {
                inner_shadow(
                    NSRect::new(
                        NSPoint::new(x + 1.0, well_bottom - 4.0),
                        NSSize::new(cw - 2.0, 4.0),
                    ),
                    -90.0,
                    0.05,
                );
            }
            NSGraphicsContext::restoreGraphicsState_class();
            // The border goes on last so the well never covers it.
            card_border().setStroke();
            path.setLineWidth(1.0);
            path.stroke();

            // The worktree plate, raised on a drop shadow.
            // The first and last rows are taller than the others; the plate
            // keeps the normal height and the extra is the well's padding.
            let room = if last { LAST_ROW_EXTRA } else { 0.0 };
            let plate = NSRect::new(
                NSPoint::new(x + PLATE_INSET + 0.5, PLATE_GAP + lead + 0.5),
                NSSize::new(
                    cw - 2.0 * PLATE_INSET - 1.0,
                    h - 2.0 * PLATE_GAP - 1.0 - room - lead,
                ),
            );
            let plate_path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                plate,
                PLATE_RADIUS,
                PLATE_RADIUS,
            );
            NSGraphicsContext::saveGraphicsState_class();
            shadow(4.0, 1.5, 0.16);
            plate_fill().setFill();
            plate_path.fill();
            NSGraphicsContext::restoreGraphicsState_class();
            // A faint highlight along the plate's top edge, like a machined bevel.
            let bevel = NSRect::new(
                NSPoint::new(plate.origin.x + 1.0, plate.origin.y + 0.5),
                NSSize::new(plate.size.width - 2.0, 1.0),
            );
            NSColor::whiteColor().colorWithAlphaComponent(0.3).setFill();
            NSBezierPath::bezierPathWithRect(bevel).fill();
            NSColor::separatorColor()
                .colorWithAlphaComponent(0.45)
                .setStroke();
            plate_path.setLineWidth(1.0);
            plate_path.stroke();
        }
    }
}
