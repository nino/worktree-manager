//! Outline-view item objects. `NSOutlineView` identifies rows by object
//! identity, so each repo, worktree and pending creation gets one long-lived
//! `WTMItem` that survives model updates — that is what preserves expansion
//! state and lets `reloadItem:` refresh a single row in place.

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::NSObject;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Repo { repo_id: String },
    Worktree { repo_id: String, path: String },
    Pending { repo_id: String, id: u64 },
}

impl ItemKind {
    /// Stable cache key.
    pub fn key(&self) -> String {
        match self {
            ItemKind::Repo { repo_id } => format!("r:{repo_id}"),
            ItemKind::Worktree { path, .. } => format!("w:{path}"),
            ItemKind::Pending { id, .. } => format!("p:{id}"),
        }
    }

    pub fn repo_id(&self) -> &str {
        match self {
            ItemKind::Repo { repo_id }
            | ItemKind::Worktree { repo_id, .. }
            | ItemKind::Pending { repo_id, .. } => repo_id,
        }
    }
}

pub struct ItemIvars {
    pub kind: ItemKind,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMItem"]
    #[ivars = ItemIvars]
    pub struct WTMItem;
);

impl WTMItem {
    pub fn new(kind: ItemKind, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ItemIvars { kind });
        unsafe { msg_send![super(this), init] }
    }

    pub fn kind(&self) -> ItemKind {
        self.ivars().kind.clone()
    }
}
