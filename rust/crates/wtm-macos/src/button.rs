//! The buttons in rows. AppKit leaves the controls inside a table's cells out
//! of the window's key view loop, and buttons only take keyboard focus when
//! Full Keyboard Access is on. These buttons always take focus, and ask the
//! controller who comes before and after them (see `controller::key_view`),
//! so Tab walks every button in every row.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSButton, NSImage, NSView};
use objc2_foundation::NSString;

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
