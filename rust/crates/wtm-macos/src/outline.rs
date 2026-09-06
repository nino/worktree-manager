//! An `NSOutlineView` whose disclosure triangle sits inside the repo cards
//! drawn by `RowView`, rather than in a gutter to their left. Cell views are
//! not moved (the outline lays those out itself); the cells' leading insets
//! account for the chevron instead (`cells.rs`).

use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSOutlineView;
use objc2_foundation::{NSInteger, NSPoint, NSRect};

use crate::rowview::CARD_MARGIN;

/// How far the disclosure triangle moves in from the row's leading edge.
pub const CHEVRON_SHIFT: f64 = CARD_MARGIN + 6.0;
/// Where cell content should start so it clears the shifted chevron.
pub const CONTENT_START: f64 = CHEVRON_SHIFT + 22.0;

define_class!(
    #[unsafe(super(NSOutlineView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMOutlineView"]
    pub struct OutlineView;

    impl OutlineView {
        #[unsafe(method(frameOfOutlineCellAtRow:))]
        fn frame_of_outline_cell(&self, row: NSInteger) -> NSRect {
            let r: NSRect = unsafe { msg_send![super(self), frameOfOutlineCellAtRow: row] };
            NSRect::new(NSPoint::new(r.origin.x + CHEVRON_SHIFT, r.origin.y), r.size)
        }
    }
);

impl OutlineView {
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![mtm.alloc::<Self>(), initWithFrame: frame] }
    }
}
