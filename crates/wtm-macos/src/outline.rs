//! An `NSOutlineView` whose disclosure triangle sits inside the repo cards
//! drawn by `RowView`, rather than in a gutter to their left. Cell views are
//! not moved (the outline lays those out itself); the cells' leading insets
//! account for the chevron instead (`cells.rs`).

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSEventType, NSOutlineView};
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

        /// Space opens the selected worktree's branch picker, the way Return
        /// opens a row's default action elsewhere on the system.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &objc2_app_kit::NSEvent) {
            let space = event.charactersIgnoringModifiers()
                .map(|c| c.to_string() == " ")
                .unwrap_or(false);
            let mtm = MainThreadMarker::from(self);
            if space && crate::controller::open_selected_picker(mtm) {
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        /// The outline is the window's key view for the whole tree, but
        /// nothing acts on it directly: focus arriving by Tab is passed on to
        /// the first button in the rows (the last one when tabbing backwards).
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            let mtm = MainThreadMarker::from(self);
            let tab = NSApplication::sharedApplication(mtm)
                .currentEvent()
                .filter(|e| e.r#type() == NSEventType::KeyDown && e.keyCode() == 48)
                .map(|e| !e.modifierFlags().contains(NSEventModifierFlags::Shift));
            if let Some(forward) = tab {
                // Not while the first-responder change is still in progress.
                DispatchQueue::main().exec_async(move || {
                    let mtm = MainThreadMarker::new().expect("main queue");
                    crate::controller::focus_first_row_control(forward, mtm);
                });
            }
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }
    }
);

impl OutlineView {
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![mtm.alloc::<Self>(), initWithFrame: frame] }
    }
}
