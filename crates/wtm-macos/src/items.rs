//! Outline-view item objects. `NSOutlineView` identifies rows by object
//! identity, so each section and row of the view's `TreeList` gets one
//! long-lived `WTMItem`, found again by its key, that survives every render:
//! that is what preserves expansion state and lets `reloadItem:` refresh a
//! single row in place.

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::NSObject;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A repo card's header: a section of the list.
    Header,
    Worktree,
    Pending,
}

pub struct ItemIvars {
    pub kind: ItemKind,
    /// The section or row key.
    pub key: String,
    /// The key of the section the row belongs to (its own, for a header).
    pub section: String,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMItem"]
    #[ivars = ItemIvars]
    pub struct WTMItem;
);

impl WTMItem {
    pub fn new(kind: ItemKind, key: &str, section: &str, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ItemIvars {
            kind,
            key: key.to_string(),
            section: section.to_string(),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn kind(&self) -> ItemKind {
        self.ivars().kind
    }

    pub fn key(&self) -> &str {
        &self.ivars().key
    }

    pub fn section(&self) -> &str {
        &self.ivars().section
    }
}
