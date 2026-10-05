//! The main window as a backend: brings the window, its toolbar, the
//! outline of repo cards, the notice bar and the empty state in line with
//! each [`View`] the shared UI renders, and reports what the user does there
//! through the view's handlers and [`host_event`].
//!
//! The outline is never reloaded wholesale for an ordinary change: each
//! render is diffed against the last by row key, and rows are inserted,
//! removed, moved and reloaded in place (`wtm_core::splice`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimationContext, NSApplication, NSApplicationDelegate, NSBackingStoreType, NSColor,
    NSControl, NSControlSize, NSControlTextEditingDelegate, NSDragOperation, NSDraggingInfo,
    NSDraggingSession, NSLayoutAttribute, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultHigh, NSLayoutPriorityDefaultLow, NSMenuItem, NSMenuItemValidation,
    NSOutlineView, NSOutlineViewDataSource, NSOutlineViewDelegate, NSPasteboard, NSPasteboardItem,
    NSPasteboardTypeString, NSPasteboardWriting, NSProgressIndicator, NSProgressIndicatorStyle,
    NSScreen, NSScrollView, NSSearchField, NSSearchFieldDelegate, NSSearchToolbarItem, NSStackView,
    NSStackViewDistribution, NSTableColumn, NSTableViewAnimationOptions,
    NSTableViewColumnAutoresizingStyle, NSTableViewSelectionHighlightStyle, NSTableViewStyle,
    NSTextFieldDelegate, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode,
    NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem, NSToolbarItemIdentifier,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView, NSWindow,
    NSWindowDelegate, NSWindowStyleMask, NSWindowToolbarStyle,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSObject, NSObjectProtocol,
    NSPoint, NSRect, NSSize, NSUserDefaults, NSURL,
};
use wtm_core::splice::{moves, splice, Splice};
use wtm_core::WindowFrame;
use wtm_toolkit::{
    flush, host_event, Effect, Element, Frame, HostEvent, MainWindow, PendingRow, RepoHeader,
    RowContent, ToolItem, TreeList, View, WorktreeRow, BRANCH_BUTTON,
};

use crate::button::Button;
use crate::cells::{
    PendingCell, PlateCell, RepoCell, WorktreeCell, PENDING_ROW_HEIGHT, REPO_ROW_HEIGHT,
    WORKTREE_ROW_HEIGHT,
};
use crate::elements::{same_shape, symbol_name, Built};
use crate::items::{ItemKind, WTMItem};
use crate::outline::OutlineView;
use crate::rowview::{RowStyle, RowView, LAST_ROW_EXTRA, WELL_LEAD};
use crate::util::{ns, render, symbol};

static CONTROLLER: OnceLock<MainThreadBound<Retained<Controller>>> = OnceLock::new();

pub fn controller(mtm: MainThreadMarker) -> Option<&'static Retained<Controller>> {
    CONTROLLER.get().map(|c| c.get(mtm))
}

pub fn main_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    controller(mtm).and_then(|c| c.ivars().window.borrow().clone())
}

thread_local! {
    /// The list as last rendered. Row buttons look their handlers up here
    /// when pressed (see `cells.rs`).
    static LIST: RefCell<TreeList> = RefCell::new(TreeList::default());
}

/// Run `f` with the header of section `key` as last rendered.
pub fn with_header(key: &str, f: impl FnOnce(&RepoHeader)) {
    let header = LIST.with(|l| l.borrow().section(key).map(|s| s.header.clone()));
    if let Some(h) = header {
        f(&h)
    }
}

/// Run `f` with worktree row `key` as last rendered.
pub fn with_worktree(key: &str, f: impl FnOnce(&WorktreeRow)) {
    let row = LIST.with(|l| l.borrow().worktree(key).cloned());
    if let Some(w) = row {
        f(&w)
    }
}

/// Run `f` with pending row `key` as last rendered.
pub fn with_pending(key: &str, f: impl FnOnce(&PendingRow)) {
    let row = LIST.with(|l| match l.borrow().row(key).map(|r| &r.content) {
        Some(RowContent::Pending(p)) => Some(p.clone()),
        _ => None,
    });
    if let Some(p) = row {
        f(&p)
    }
}

/// How long after a collapse the card is checked to be closed: the outline's
/// row animation is 0.25s.
const COLLAPSE_SETTLE_NS: i64 = 400_000_000;

/// Where AppKit kept the window's frame for versions before `ui-state.json`,
/// which set the frame autosave name `WTMMainWindow`.
const LEGACY_FRAME_KEY: &str = "NSWindow Frame WTMMainWindow";

/// The pasteboard type of a repo dragged to a new place in the list: its
/// key. Private to this app, so nothing else takes the drop.
const REPO_DRAG_TYPE: &str = "uk.org.plinth.worktree-manager.repo";

/// Toolbar item identifiers are the view's ids under this prefix.
const TOOLBAR_PREFIX: &str = "wtm.";

/// The tree the outline currently shows: section items in order, each with
/// its rows.
#[derive(Default)]
struct Tree {
    roots: Vec<Retained<WTMItem>>,
    children: HashMap<String, Vec<Retained<WTMItem>>>,
}

/// An element tree shown in a container, as last rendered.
struct Shown {
    element: Element,
    built: Built,
}

pub struct ControllerIvars {
    window: RefCell<Option<Retained<NSWindow>>>,
    outline: RefCell<Option<Retained<NSOutlineView>>>,
    column: RefCell<Option<Retained<NSTableColumn>>>,
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    search: RefCell<Option<Retained<NSSearchField>>>,
    notice_bar: RefCell<Option<Retained<NSView>>>,
    notice: RefCell<Option<Shown>>,
    empty_box: RefCell<Option<Retained<NSView>>>,
    empty: RefCell<Option<Shown>>,
    activity: RefCell<Option<Retained<NSProgressIndicator>>>,
    toolbar: RefCell<Vec<ToolItem>>,
    items: RefCell<HashMap<String, Retained<WTMItem>>>,
    tree: RefCell<Tree>,
    /// The section being dragged to a new place in the list, from the start
    /// of the drag to its end.
    dragged: RefCell<Option<String>>,
    /// Sections whose cards are closed, as the outline shows them.
    collapsed: RefCell<HashSet<String>>,
    /// A render is in progress: what the outline and the search field report
    /// now is the render's doing, not the user's, and is not passed on.
    rendering: Cell<bool>,
    /// The window's frame from before it went full screen, reported in place
    /// of the full-screen one until it leaves: the next launch opens a
    /// window, and a window the size of the screen is not the one to open.
    frame_before_full_screen: Cell<Option<NSRect>>,
    /// The program's first render has not happened: `Started` waits for
    /// AppKit to finish launching.
    start: RefCell<Option<Box<dyn FnOnce(Vec<Frame>)>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMController"]
    #[ivars = ControllerIvars]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    impl Controller {
        #[unsafe(method(toolbarAction:))]
        fn toolbar_action(&self, sender: Option<&AnyObject>) {
            let id = sender
                .and_then(|s| s.downcast_ref::<NSToolbarItem>())
                .map(|i| i.itemIdentifier().to_string());
            if let Some(id) = id {
                self.press_tool(&id);
            }
        }

        #[unsafe(method(menuAction:))]
        fn menu_action(&self, sender: Option<&AnyObject>) {
            if let Some(item) = sender.and_then(|s| s.downcast_ref::<NSMenuItem>()) {
                crate::menu::perform(item.tag());
            }
        }

        /// The clip view changed size: make the single column exactly that
        /// wide, so rows never extend past the visible area.
        #[unsafe(method(clipFrameChanged:))]
        fn clip_frame_changed(&self, _n: &NSNotification) {
            self.fit_column();
        }

        /// The list was scrolled, by whatever means.
        #[unsafe(method(clipBoundsChanged:))]
        fn clip_bounds_changed(&self, _n: &NSNotification) {
            self.report_scroll();
        }
    }

    unsafe impl NSMenuItemValidation for Controller {
        /// The shared UI decides what is enabled. Whatever is still queued
        /// (an arrow key's new selection) is run first, so ⌘N straight after
        /// the arrow acts on the row the arrow went to.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            crate::menu::validate(item.tag(), flush)
        }
    }

    // MARK: Application lifecycle

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = MainThreadMarker::from(self);
            let screens = NSScreen::screens(mtm)
                .iter()
                .map(|s| frame(s.visibleFrame()))
                .collect();
            let start = self.ivars().start.borrow_mut().take();
            if let Some(start) = start {
                start(screens);
            }
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _n: &NSNotification) {
            host_event(HostEvent::Activated);
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _a: &NSApplication) -> bool {
            true
        }

        /// Last chance to write the window state, so it is run now rather
        /// than on a turn of the run loop that will never come.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            self.report_frame();
            host_event(HostEvent::WillQuit);
            flush();
        }

        #[unsafe(method(applicationSupportsSecureRestorableState:))]
        fn secure_restorable(&self, _a: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _a: &NSApplication, urls: &NSArray<NSURL>) {
            let paths: Vec<std::path::PathBuf> =
                urls.iter().filter_map(|u| u.path().map(|p| std::path::PathBuf::from(p.to_string()))).collect();
            if !paths.is_empty() {
                host_event(HostEvent::OpenPaths(paths));
            }
        }
    }

    unsafe impl NSWindowDelegate for Controller {
        /// Both fire once per frame of a live resize or drag; the core
        /// coalesces the writes.
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            self.report_frame();
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _n: &NSNotification) {
            self.report_frame();
        }

        /// Before the transition starts, so none of its frames is reported.
        #[unsafe(method(windowWillEnterFullScreen:))]
        fn window_will_enter_full_screen(&self, _n: &NSNotification) {
            let frame = self.window().map(|w| w.frame());
            self.ivars().frame_before_full_screen.set(frame);
        }

        #[unsafe(method(windowDidFailToEnterFullScreen:))]
        fn window_did_fail_to_enter_full_screen(&self, _w: &NSWindow) {
            self.ivars().frame_before_full_screen.set(None);
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _n: &NSNotification) {
            self.ivars().frame_before_full_screen.set(None);
            self.report_frame();
        }
    }

    // MARK: Toolbar

    unsafe impl NSToolbarDelegate for Controller {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(&self, _t: &NSToolbar, ident: &NSToolbarItemIdentifier, _flag: bool) -> Option<Retained<NSToolbarItem>> {
            self.make_toolbar_item(ident)
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn default_items(&self, _t: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            self.toolbar_identifiers()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed_items(&self, _t: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            self.toolbar_identifiers()
        }
    }

    // MARK: Search

    unsafe impl NSControlTextEditingDelegate for Controller {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, n: &NSNotification) {
            self.search_changed(n);
        }
    }
    unsafe impl NSTextFieldDelegate for Controller {}
    unsafe impl NSSearchFieldDelegate for Controller {}

    // MARK: Outline data source

    unsafe impl NSOutlineViewDataSource for Controller {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        unsafe fn number_of_children(&self, _o: &NSOutlineView, item: Option<&AnyObject>) -> NSInteger {
            let tree = self.ivars().tree.borrow();
            match item.and_then(|i| i.downcast_ref::<WTMItem>()) {
                None => tree.roots.len() as NSInteger,
                Some(i) => tree.children.get(i.key()).map(|c| c.len()).unwrap_or(0) as NSInteger,
            }
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        unsafe fn child(&self, _o: &NSOutlineView, index: NSInteger, item: Option<&AnyObject>) -> Retained<AnyObject> {
            let tree = self.ivars().tree.borrow();
            let child = match item.and_then(|i| i.downcast_ref::<WTMItem>()) {
                None => tree.roots[index as usize].clone(),
                Some(i) => tree.children[i.key()][index as usize].clone(),
            };
            Retained::into_super(Retained::into_super(child))
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        unsafe fn is_expandable(&self, _o: &NSOutlineView, item: &AnyObject) -> bool {
            item.downcast_ref::<WTMItem>().is_some_and(|i| i.kind() == ItemKind::Header)
        }

        /// Repos can be dragged to a new place in the list; worktrees keep
        /// their sorted order.
        #[unsafe(method_id(outlineView:pasteboardWriterForItem:))]
        unsafe fn pasteboard_writer(&self, _o: &NSOutlineView, item: &AnyObject) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            repo_drag_writer(item)
        }

        #[unsafe(method(outlineView:draggingSession:willBeginAtPoint:forItems:))]
        unsafe fn drag_will_begin(&self, _o: &NSOutlineView, _s: &NSDraggingSession, _p: NSPoint, items: &NSArray) {
            *self.ivars().dragged.borrow_mut() = items
                .firstObject()
                .and_then(|i| i.downcast_ref::<WTMItem>().map(|i| i.key().to_string()));
        }

        #[unsafe(method(outlineView:draggingSession:endedAtPoint:operation:))]
        fn drag_ended(&self, _o: &NSOutlineView, _s: &NSDraggingSession, _p: NSPoint, _op: NSDragOperation) {
            *self.ivars().dragged.borrow_mut() = None;
        }

        #[unsafe(method(outlineView:validateDrop:proposedItem:proposedChildIndex:))]
        unsafe fn validate_drop(&self, outline: &NSOutlineView, info: &ProtocolObject<dyn NSDraggingInfo>, _item: Option<&AnyObject>, _index: NSInteger) -> NSDragOperation {
            self.validate_repo_drop(outline, info)
        }

        /// The gap is worked out again rather than taken from AppKit: the
        /// tree may have changed since the drag last moved.
        #[unsafe(method(outlineView:acceptDrop:item:childIndex:))]
        unsafe fn accept_drop(&self, outline: &NSOutlineView, info: &ProtocolObject<dyn NSDraggingInfo>, _item: Option<&AnyObject>, _index: NSInteger) -> bool {
            self.accept_repo_drop(outline, info)
        }
    }

    // MARK: Outline delegate

    unsafe impl NSOutlineViewDelegate for Controller {
        #[unsafe(method_id(outlineView:viewForTableColumn:item:))]
        unsafe fn view_for_item(&self, outline: &NSOutlineView, _c: Option<&NSTableColumn>, item: &AnyObject) -> Option<Retained<NSView>> {
            self.make_cell_view(outline, item)
        }

        #[unsafe(method(outlineView:heightOfRowByItem:))]
        unsafe fn height_of_row(&self, outline: &NSOutlineView, item: &AnyObject) -> f64 {
            match item.downcast_ref::<WTMItem>() {
                Some(item) => self.row_height(outline, item),
                None => PENDING_ROW_HEIGHT,
            }
        }

        /// The keyboard's cursor moved.
        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn selection_did_change(&self, _n: &NSNotification) {
            self.report_selection();
        }

        #[unsafe(method(outlineView:shouldSelectItem:))]
        unsafe fn should_select(&self, _o: &NSOutlineView, _item: &AnyObject) -> bool {
            // Selection is the keyboard's cursor through the tree: the arrow
            // keys move it, ⌘N and ⌘T act on it, and the row draws it as a
            // glowing outline (see `rowview::draw_selection`).
            true
        }

        /// A drag that rests on a closed card makes the outline open it, and
        /// a drop leaves it open; neither is anything the user asked for.
        #[unsafe(method(outlineView:shouldExpandItem:))]
        unsafe fn should_expand(&self, _o: &NSOutlineView, item: &AnyObject) -> bool {
            !self.closed_card_under_drag(item)
        }

        #[unsafe(method_id(outlineView:rowViewForItem:))]
        unsafe fn row_view_for_item(&self, outline: &NSOutlineView, item: &AnyObject) -> Option<Retained<objc2_app_kit::NSTableRowView>> {
            self.make_row_view(outline, item)
        }

        /// The card opens on the first frame of the expansion: its header
        /// loses the rounded bottom as the rows start unrolling beneath it.
        #[unsafe(method(outlineViewItemWillExpand:))]
        fn will_expand(&self, n: &NSNotification) {
            if let Some(key) = expanded_section(n) {
                self.ivars().collapsed.borrow_mut().remove(&key);
                self.sync_header_style(&key, None);
                self.report_toggle(key, true);
            }
        }

        /// The card stays open while its rows slide away; it closes when the
        /// last of them leaves the outline (see `child_row_leaving`).
        #[unsafe(method(outlineViewItemWillCollapse:))]
        fn will_collapse(&self, n: &NSNotification) {
            if let Some(key) = expanded_section(n) {
                self.ivars().collapsed.borrow_mut().insert(key.clone());
                self.report_toggle(key, false);
            }
            self.snapshot_collapsing_rows(n);
        }

        #[unsafe(method(outlineViewItemDidExpand:))]
        fn did_expand(&self, _n: &NSNotification) {
            self.sync_row_styles_later();
        }

        /// The card normally closes on the frame its last row is dropped
        /// (`child_row_leaving`). This is the backstop for anything that
        /// leaves it open — a collapse the outline chose not to animate, or
        /// rows dropped in an order that hook does not see — so a card can
        /// never stay drawn open with nothing under it.
        #[unsafe(method(outlineViewItemDidCollapse:))]
        fn did_collapse(&self, n: &NSNotification) {
            let Some(key) = expanded_section(n) else { return };
            let _ = DispatchQueue::main().after(
                dispatch2::DispatchTime::NOW.time(COLLAPSE_SETTLE_NS),
                move || {
                    let mtm = MainThreadMarker::new().expect("main queue");
                    if let Some(c) = controller(mtm) {
                        c.sync_header_style(&key, None);
                    }
                },
            );
        }
    }
);

// MARK: Keyboard focus through the rows

/// The focusable buttons of one row's cell, in reading order.
fn row_controls(outline: &NSOutlineView, row: NSInteger, make: bool) -> Vec<Retained<NSView>> {
    fn collect(view: &NSView, out: &mut Vec<Retained<NSView>>) {
        for sv in view.subviews().iter() {
            if let Some(b) = sv.downcast_ref::<Button>() {
                if b.isEnabled() && !b.isHiddenOrHasHiddenAncestor() {
                    out.push(sv.clone());
                }
            } else {
                collect(&sv, out);
            }
        }
    }
    let mut out = Vec::new();
    if make {
        outline.scrollRowToVisible(row);
    }
    if let Some(cell) = outline.viewAtColumn_row_makeIfNecessary(0, row, make) {
        collect(&cell, &mut out);
    }
    out
}

/// The key view before or after `from`, one of the row buttons: the next
/// button in its row, else the first in a following row (brought into view),
/// else whatever follows the outline in the window's loop. `None` when
/// `from` is not in the list (a button in a sheet or the Settings window),
/// which keeps AppKit's own loop.
pub fn key_view(from: &NSView, forward: bool) -> Option<Option<Retained<NSView>>> {
    let mtm = MainThreadMarker::new()?;
    let outline = controller(mtm)?.ivars().outline.borrow().clone()?;
    let row = outline.rowForView(from);
    if row < 0 {
        return None;
    }
    let controls = row_controls(&outline, row, false);
    let idx = controls
        .iter()
        .position(|c| std::ptr::eq(Retained::as_ptr(c), from))?;
    let n = outline.numberOfRows();
    Some(if forward {
        if let Some(c) = controls.get(idx + 1) {
            return Some(Some(c.clone()));
        }
        for r in row + 1..n {
            if let Some(c) = row_controls(&outline, r, true).into_iter().next() {
                return Some(Some(c));
            }
        }
        unsafe { outline.nextValidKeyView() }
    } else {
        if idx > 0 {
            return Some(Some(controls[idx - 1].clone()));
        }
        for r in (0..row).rev() {
            if let Some(c) = row_controls(&outline, r, true).into_iter().last() {
                return Some(Some(c));
            }
        }
        unsafe { outline.previousValidKeyView() }
    })
}

/// Space on the selected row. Returns whether the row took it: only a
/// worktree row has something Space does (its branch picker), and on any
/// other the key goes on to the outline.
pub fn activate_selected(mtm: MainThreadMarker) -> bool {
    let Some(c) = controller(mtm) else {
        return false;
    };
    let Some((_, item)) = c.selected_item() else {
        return false;
    };
    if item.kind() != ItemKind::Worktree {
        return false;
    }
    let handler = LIST.with(|l| l.borrow().on_activate.clone());
    handler.call(item.key().to_string());
    true
}

/// Put the keyboard back on the tree, so the arrow keys move the selection
/// again — where focus belongs after a popover or a row control is done.
pub fn focus_tree(mtm: MainThreadMarker) {
    let Some(c) = controller(mtm) else { return };
    if let (Some(window), Some(outline)) = (c.window(), c.ivars().outline.borrow().clone()) {
        window.makeFirstResponder(Some(&outline));
    }
}

/// Tab landed on the outline: move focus to the first row button (or the
/// last, tabbing backwards).
pub fn focus_first_row_control(forward: bool, mtm: MainThreadMarker) {
    let Some(c) = controller(mtm) else { return };
    let (Some(outline), Some(window)) = (c.ivars().outline.borrow().clone(), c.window()) else {
        return;
    };
    let n = outline.numberOfRows();
    let rows: Box<dyn Iterator<Item = NSInteger>> = if forward {
        Box::new(0..n)
    } else {
        Box::new((0..n).rev())
    };
    for r in rows {
        let controls = row_controls(&outline, r, true);
        let pick = if forward {
            controls.first()
        } else {
            controls.last()
        };
        if let Some(v) = pick {
            window.makeFirstResponder(Some(v));
            return;
        }
    }
}

/// The view a popover anchored at `(key, id)` hangs from, brought into view.
pub fn anchor(key: &str, id: &str, mtm: MainThreadMarker) -> Option<Retained<NSView>> {
    let c = controller(mtm)?;
    let outline = c.ivars().outline.borrow().clone()?;
    let item = c.ivars().items.borrow().get(key).cloned()?;
    let row = unsafe { outline.rowForItem(Some(&item)) };
    if row < 0 {
        return None;
    }
    outline.scrollRowToVisible(row);
    let cell = outline.viewAtColumn_row_makeIfNecessary(0, row, true)?;
    match id {
        BRANCH_BUTTON => cell
            .downcast_ref::<WorktreeCell>()
            .map(|c| c.branch_button()),
        _ => Some(cell),
    }
}

/// Whether any child row view of section `key`'s card is still in the
/// outline (it no longer has the rows, but keeps their views while they
/// slide away). `except` is a row on its way out: `viewWillMoveToSuperview:`
/// runs before the view is actually removed, so the last one to leave would
/// otherwise still count itself and the card would never close.
fn rows_lingering(outline: &NSOutlineView, key: &str, except: Option<&RowView>) -> bool {
    let is_child_of = |v: &NSView| {
        v.downcast_ref::<RowView>()
            .map(|r| {
                !except.map(|e| std::ptr::eq(e, r)).unwrap_or(false)
                    && matches!(r.style(), RowStyle::Child { .. })
                    && r.owner() == key
            })
            .unwrap_or(false)
    };
    outline.subviews().iter().any(|v| {
        if v.downcast_ref::<RowView>().is_some() {
            is_child_of(&v)
        } else {
            // The outline's clip view for rows sliding away.
            v.subviews().iter().any(|w| is_child_of(&w))
        }
    })
}

/// Called by a child row view as it leaves the outline: if it was the last
/// of its card's rows, the header can now draw its closed bottom edge.
pub fn child_row_leaving(row: &RowView) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(c) = controller(mtm) else {
        return;
    };
    let key = row.owner();
    if c.ivars().collapsed.borrow().contains(&key) {
        c.sync_header_style(&key, Some(row));
    }
}

fn set_cell_lead(cell: &NSView, first: bool) {
    let lead = if first { WELL_LEAD } else { 0.0 };
    if let Some(c) = cell.downcast_ref::<WorktreeCell>() {
        c.set_lead(lead);
    } else if let Some(c) = cell.downcast_ref::<PendingCell>() {
        c.set_lead(lead);
    }
}

/// The section key carried by an expand/collapse notification.
fn expanded_section(n: &NSNotification) -> Option<String> {
    let info = n.userInfo()?;
    let obj = info.objectForKey(&*ns("NSObject"))?;
    let item = obj.downcast_ref::<WTMItem>()?;
    (item.kind() == ItemKind::Header).then(|| item.key().to_string())
}

fn copy_to_pasteboard(text: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    unsafe { pb.setString_forType(&ns(text), NSPasteboardTypeString) };
}

impl Controller {
    pub fn new(start: impl FnOnce(Vec<Frame>) + 'static, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ControllerIvars {
            window: RefCell::new(None),
            outline: RefCell::new(None),
            column: RefCell::new(None),
            scroll: RefCell::new(None),
            search: RefCell::new(None),
            notice_bar: RefCell::new(None),
            notice: RefCell::new(None),
            empty_box: RefCell::new(None),
            empty: RefCell::new(None),
            activity: RefCell::new(None),
            toolbar: RefCell::new(Vec::new()),
            items: RefCell::new(HashMap::new()),
            tree: RefCell::new(Tree::default()),
            dragged: RefCell::new(None),
            collapsed: RefCell::new(HashSet::new()),
            rendering: Cell::new(false),
            frame_before_full_screen: Cell::new(None),
            start: RefCell::new(Some(Box::new(start))),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let _ = CONTROLLER.set(MainThreadBound::new(this.clone(), mtm));
        this
    }

    fn window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().window.borrow().clone()
    }

    // MARK: Reporting

    fn report_frame(&self) {
        let Some(window) = self.window() else { return };
        let f = self
            .ivars()
            .frame_before_full_screen
            .get()
            .unwrap_or_else(|| window.frame());
        host_event(HostEvent::FrameChanged(frame(f)));
    }

    fn report_scroll(&self) {
        if let Some(scroll) = self.ivars().scroll.borrow().as_ref() {
            host_event(HostEvent::Scrolled(scroll.contentView().bounds().origin.y));
        }
    }

    fn report_selection(&self) {
        if self.ivars().rendering.get() {
            return;
        }
        let key = self.selected_item().map(|(_, i)| i.key().to_string());
        let handler = LIST.with(|l| l.borrow().on_select.clone());
        handler.call(key);
    }

    fn report_toggle(&self, key: String, open: bool) {
        if self.ivars().rendering.get() {
            return;
        }
        let handler = LIST.with(|l| l.borrow().on_toggle.clone());
        handler.call((key, open));
    }

    fn search_changed(&self, n: &NSNotification) {
        if self.ivars().rendering.get() {
            return;
        }
        let Some(obj) = n.object() else { return };
        let Some(field) = obj.downcast_ref::<NSControl>() else {
            return;
        };
        let value = field.stringValue().to_string();
        let handler = self.ivars().toolbar.borrow().iter().find_map(|t| match t {
            ToolItem::Search { on_change, .. } => Some(on_change.clone()),
            _ => None,
        });
        if let Some(h) = handler {
            h.call(value);
        }
    }

    fn press_tool(&self, identifier: &str) {
        let Some(id) = identifier.strip_prefix(TOOLBAR_PREFIX) else {
            return;
        };
        let handler = self.ivars().toolbar.borrow().iter().find_map(|t| match t {
            ToolItem::Button {
                id: i, on_press, ..
            } if *i == id => Some(on_press.clone()),
            _ => None,
        });
        if let Some(h) = handler {
            h.call(());
        }
    }

    // MARK: Toolbar

    fn toolbar_identifiers(&self) -> Retained<NSArray<NSToolbarItemIdentifier>> {
        let flexible = unsafe { NSToolbarFlexibleSpaceItemIdentifier }.to_string();
        let ids: Vec<_> = self
            .ivars()
            .toolbar
            .borrow()
            .iter()
            .map(|t| match t {
                ToolItem::Button { id, .. } | ToolItem::Search { id, .. } => {
                    ns(&format!("{TOOLBAR_PREFIX}{id}"))
                }
                ToolItem::Flex => ns(&flexible),
            })
            .collect();
        NSArray::from_retained_slice(&ids)
    }

    fn make_toolbar_item(
        &self,
        ident: &NSToolbarItemIdentifier,
    ) -> Option<Retained<NSToolbarItem>> {
        let mtm = MainThreadMarker::from(self);
        let id = ident.to_string();
        let id = id.strip_prefix(TOOLBAR_PREFIX)?;
        let tool = self
            .ivars()
            .toolbar
            .borrow()
            .iter()
            .find(|t| match t {
                ToolItem::Button { id: i, .. } | ToolItem::Search { id: i, .. } => *i == id,
                ToolItem::Flex => false,
            })?
            .clone();
        match tool {
            ToolItem::Button { label, icon, .. } => {
                let item = NSToolbarItem::initWithItemIdentifier(mtm.alloc(), ident);
                item.setLabel(&ns(&label));
                item.setToolTip(Some(&ns(&label)));
                item.setImage(symbol(symbol_name(icon), &label).as_deref());
                item.setBordered(true);
                unsafe {
                    item.setTarget(Some(self.as_ref()));
                    item.setAction(Some(sel!(toolbarAction:)));
                }
                Some(item)
            }
            ToolItem::Search {
                value, placeholder, ..
            } => {
                let item = NSSearchToolbarItem::initWithItemIdentifier(mtm.alloc(), ident);
                item.setPreferredWidthForSearchField(220.0);
                let field = item.searchField();
                field.setPlaceholderString(Some(&ns(&placeholder)));
                field.setStringValue(&ns(&value));
                field.setSendsSearchStringImmediately(true);
                field.setSendsWholeSearchString(false);
                unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
                *self.ivars().search.borrow_mut() = Some(field);
                Some(Retained::into_super(item))
            }
            ToolItem::Flex => None,
        }
    }

    // MARK: Rows

    fn row_height(&self, outline: &NSOutlineView, item: &WTMItem) -> f64 {
        // Heights never change on expand or collapse: the header is one
        // height open or closed, and a card's first and last rows carry the
        // well's padding — known when they are inserted.
        let base = match item.kind() {
            ItemKind::Header => return REPO_ROW_HEIGHT,
            ItemKind::Worktree => WORKTREE_ROW_HEIGHT,
            ItemKind::Pending => PENDING_ROW_HEIGHT,
        };
        match self.row_style(outline, item) {
            RowStyle::Child { first, last } => {
                base + if first { WELL_LEAD } else { 0.0 } + if last { LAST_ROW_EXTRA } else { 0.0 }
            }
            RowStyle::Header { .. } => base,
        }
    }

    /// The card slice a row should draw, from the display tree and expansion.
    fn row_style(&self, outline: &NSOutlineView, item: &WTMItem) -> RowStyle {
        self.row_style_excluding(outline, item, None)
    }

    /// As `row_style`, ignoring a row that is on its way out of the outline.
    fn row_style_excluding(
        &self,
        outline: &NSOutlineView,
        item: &WTMItem,
        leaving: Option<&RowView>,
    ) -> RowStyle {
        let tree = self.ivars().tree.borrow();
        let section = item.section();
        match item.kind() {
            ItemKind::Header => {
                let has_children = tree
                    .children
                    .get(section)
                    .map(|c| !c.is_empty())
                    .unwrap_or(false);
                // A collapsing card is still open while its rows are on their
                // way out: the outline keeps their views until the slide ends.
                let open = has_children
                    && (!self.ivars().collapsed.borrow().contains(section)
                        || rows_lingering(outline, section, leaving));
                RowStyle::Header { closed: !open }
            }
            ItemKind::Worktree | ItemKind::Pending => {
                let children = tree.children.get(section);
                let is = |c: Option<&Retained<WTMItem>>| {
                    c.map(|l| std::ptr::eq(Retained::as_ptr(l), item))
                        .unwrap_or(false)
                };
                RowStyle::Child {
                    first: is(children.and_then(|c| c.first())),
                    last: is(children.and_then(|c| c.last())),
                }
            }
        }
    }

    fn make_row_view(
        &self,
        outline: &NSOutlineView,
        item: &AnyObject,
    ) -> Option<Retained<objc2_app_kit::NSTableRowView>> {
        let mtm = MainThreadMarker::from(self);
        let item = item.downcast_ref::<WTMItem>()?;
        let style = self.row_style(outline, item);
        let view =
            match unsafe { outline.makeViewWithIdentifier_owner(&ns(RowView::IDENTIFIER), None) } {
                Some(v) => v.downcast::<RowView>().ok()?,
                None => {
                    let v = RowView::new(style, mtm);
                    v.setIdentifier(Some(&ns(RowView::IDENTIFIER)));
                    v
                }
            };
        view.set_snapshot(None);
        view.set_owner(item.section());
        view.set_style(style);
        Some(Retained::into_super(view))
    }

    /// Re-tag one card's header row, e.g. when the card opens or closes.
    /// Only a redraw: header heights never change.
    fn sync_header_style(&self, key: &str, leaving: Option<&RowView>) {
        let Some(outline) = self.ivars().outline.borrow().clone() else {
            return;
        };
        let Some((item, row)) = self.row_of(&outline, key) else {
            return;
        };
        let style = self.row_style_excluding(&outline, &item, leaving);
        if let Some(v) = outline.rowViewAtRow_makeIfNecessary(row, false) {
            if let Some(v) = v.downcast_ref::<RowView>() {
                if v.set_style(style) {
                    // Drawn now, not on the next pass: a card closing as its
                    // last sliding row is dropped must change in that frame.
                    v.displayIfNeeded();
                }
            }
        }
    }

    /// The rows about to slide away can no longer draw (see
    /// `RowView::set_snapshot`): freeze each one as an image first.
    fn snapshot_collapsing_rows(&self, n: &NSNotification) {
        let (Some(outline), Some(obj)) = (
            self.ivars().outline.borrow().clone(),
            n.userInfo().and_then(|i| i.objectForKey(&*ns("NSObject"))),
        ) else {
            return;
        };
        let row = unsafe { outline.rowForItem(Some(&obj)) };
        if row < 0 {
            return;
        }
        let count = obj
            .downcast_ref::<WTMItem>()
            .and_then(|i| {
                self.ivars()
                    .tree
                    .borrow()
                    .children
                    .get(i.key())
                    .map(|c| c.len())
            })
            .unwrap_or(0) as NSInteger;
        for r in row + 1..=row + count {
            if let Some(v) = outline.rowViewAtRow_makeIfNecessary(r, false) {
                if let Some(v) = v.downcast_ref::<RowView>() {
                    if let Some(rep) = render(v) {
                        v.set_snapshot(Some(rep));
                    }
                }
            }
        }
    }

    /// Expand/collapse notifications arrive mid-operation, where the outline
    /// forbids re-entrant height changes; sync on the next run-loop turn.
    fn sync_row_styles_later(&self) {
        on_next_turn(|c| c.sync_row_styles());
    }

    /// Row views are reused by the outline, so after any structural change
    /// (rows added/removed, expand/collapse) each visible row is told which
    /// card slice it now is: a former last child becomes a middle one, a
    /// collapsed header closes its card.
    fn sync_row_styles(&self) {
        self.sync_row_styles_noting(&[]);
    }

    /// As `sync_row_styles`, also noting the heights of `resized`: a row off
    /// screen has no row view to report that its slice changed.
    fn sync_row_styles_noting(&self, resized: &[Retained<WTMItem>]) {
        let Some(outline) = self.ivars().outline.borrow().clone() else {
            return;
        };
        let mut heights_changed = false;
        for row in 0..outline.numberOfRows() {
            let Some(item) = outline.itemAtRow(row) else {
                continue;
            };
            let Some(item) = item.downcast_ref::<WTMItem>() else {
                continue;
            };
            let style = self.row_style(&outline, item);
            if let Some(v) = outline.rowViewAtRow_makeIfNecessary(row, false) {
                if let Some(v) = v.downcast_ref::<RowView>() {
                    heights_changed |= v.set_style(style);
                }
            }
            if let RowStyle::Child { first, .. } = style {
                if let Some(cell) = outline.viewAtColumn_row_makeIfNecessary(0, row, false) {
                    set_cell_lead(&cell, first);
                }
            }
        }
        // A row that became (or stopped being) its card's last one changed
        // height; a zero-duration group keeps the relayout instant.
        let rows = if heights_changed {
            objc2_foundation::NSIndexSet::indexSetWithIndexesInRange(
                objc2_foundation::NSRange::new(0, outline.numberOfRows() as usize),
            )
        } else {
            let rows: Vec<usize> = resized
                .iter()
                .map(|item| unsafe { outline.rowForItem(Some(item)) })
                .filter(|&row| row >= 0)
                .map(|row| row as usize)
                .collect();
            if rows.is_empty() {
                return;
            }
            index_set(&rows)
        };
        NSAnimationContext::beginGrouping();
        NSAnimationContext::currentContext().setDuration(0.0);
        outline.noteHeightOfRowsWithIndexesChanged(&rows);
        NSAnimationContext::endGrouping();
    }

    fn make_cell_view(
        &self,
        outline: &NSOutlineView,
        item: &AnyObject,
    ) -> Option<Retained<NSView>> {
        let mtm = MainThreadMarker::from(self);
        let item = item.downcast_ref::<WTMItem>()?;
        let key = item.key();
        let cell: Retained<NSView> = match item.kind() {
            ItemKind::Header => {
                let header = LIST.with(|l| l.borrow().section(key).map(|s| s.header.clone()))?;
                let cell = match unsafe {
                    outline.makeViewWithIdentifier_owner(&ns(RepoCell::IDENTIFIER), None)
                } {
                    Some(v) => v.downcast::<RepoCell>().ok()?,
                    None => RepoCell::new(mtm),
                };
                cell.configure(key, &header);
                return Some(Retained::into_super(Retained::into_super(cell)));
            }
            ItemKind::Worktree => {
                let row = LIST.with(|l| l.borrow().worktree(key).cloned())?;
                let cell = match unsafe {
                    outline.makeViewWithIdentifier_owner(&ns(WorktreeCell::IDENTIFIER), None)
                } {
                    Some(v) => v.downcast::<WorktreeCell>().ok()?,
                    None => WorktreeCell::new(mtm),
                };
                cell.configure(key, &row);
                Retained::into_super(Retained::into_super(cell))
            }
            ItemKind::Pending => {
                let row = LIST.with(|l| match l.borrow().row(key).map(|r| &r.content) {
                    Some(RowContent::Pending(p)) => Some(p.clone()),
                    _ => None,
                })?;
                let cell = match unsafe {
                    outline.makeViewWithIdentifier_owner(&ns(PendingCell::IDENTIFIER), None)
                } {
                    Some(v) => v.downcast::<PendingCell>().ok()?,
                    None => PendingCell::new(mtm),
                };
                cell.configure(key, &row);
                Retained::into_super(Retained::into_super(cell))
            }
        };
        if let RowStyle::Child { first, .. } = self.row_style(outline, item) {
            set_cell_lead(&cell, first);
        }
        Some(cell)
    }

    fn fit_column(&self) {
        let iv = self.ivars();
        let (Some(scroll), Some(column), Some(outline)) = (
            iv.scroll.borrow().clone(),
            iv.column.borrow().clone(),
            iv.outline.borrow().clone(),
        ) else {
            return;
        };
        let width = scroll.contentSize().width;
        if width > 0.0 && (column.width() - width).abs() > 0.5 {
            column.setWidth(width);
            outline.setFrameSize(NSSize::new(width, outline.frame().size.height));
        }
    }

    /// The selected row and its item.
    fn selected_item(&self) -> Option<(NSInteger, Retained<WTMItem>)> {
        let outline = self.ivars().outline.borrow().clone()?;
        let row = outline.selectedRow();
        if row < 0 {
            return None;
        }
        let item = outline.itemAtRow(row)?;
        Some((row, item.downcast::<WTMItem>().ok()?))
    }

    /// The outline row showing `key`, and its item. `None` when it is not in
    /// the tree, or sits inside a card that is closed.
    fn row_of(&self, outline: &NSOutlineView, key: &str) -> Option<(Retained<WTMItem>, NSInteger)> {
        let item = self.ivars().items.borrow().get(key).cloned()?;
        let row = unsafe { outline.rowForItem(Some(&item)) };
        (row >= 0).then_some((item, row))
    }

    // MARK: Reordering repos

    /// `item` is a card the user closed and a repo is being dragged, which
    /// is when the outline opens cards on its own (see `should_expand`).
    /// Cards meant to be open still open, so a render landing mid-drag puts
    /// back what it rebuilds.
    fn closed_card_under_drag(&self, item: &AnyObject) -> bool {
        let iv = self.ivars();
        iv.dragged.borrow().is_some()
            && !iv.rendering.get()
            && item.downcast_ref::<WTMItem>().is_some_and(|i| {
                i.kind() == ItemKind::Header && iv.collapsed.borrow().contains(i.key())
            })
    }

    /// The gap between cards a drag at `info`'s location is over, as an
    /// index among the sections the list shows. The drop is always between
    /// cards, never into one: the upper half of a card, header and open rows
    /// together, is the gap before it, the lower half the gap after.
    fn repo_drop_gap(
        &self,
        outline: &NSOutlineView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
    ) -> usize {
        let y = outline
            .convertPoint_fromView(info.draggingLocation(), None)
            .y;
        let tree = self.ivars().tree.borrow();
        for (i, root) in tree.roots.iter().enumerate() {
            let row = unsafe { outline.rowForItem(Some(root)) };
            if row < 0 {
                continue;
            }
            let open = if unsafe { outline.isItemExpanded(Some(root)) } {
                tree.children.get(root.key()).map_or(0, Vec::len)
            } else {
                0
            };
            // The outline is flipped: y grows downwards.
            let top = outline.rectOfRow(row).origin.y;
            let last = outline.rectOfRow(row + open as NSInteger);
            if y < (top + last.origin.y + last.size.height) / 2.0 {
                return i;
            }
        }
        tree.roots.len()
    }

    /// The gap a drop at `info`'s location goes in, and the drop as the view
    /// takes it: the dragged section and the one it lands before. `None`
    /// for no drag of a section, or a drop beside the dragged one, which
    /// would change nothing.
    fn repo_drop(
        &self,
        outline: &NSOutlineView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
    ) -> Option<(usize, String, Option<String>)> {
        let dragged = self.ivars().dragged.borrow().clone()?;
        let gap = self.repo_drop_gap(outline, info);
        let tree = self.ivars().tree.borrow();
        let from = tree.roots.iter().position(|r| r.key() == dragged)?;
        if gap == from || gap == from + 1 {
            return None;
        }
        let before = tree.roots.get(gap).map(|r| r.key().to_string());
        Some((gap, dragged, before))
    }

    fn validate_repo_drop(
        &self,
        outline: &NSOutlineView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
    ) -> NSDragOperation {
        let Some((gap, _, _)) = self.repo_drop(outline, info) else {
            return NSDragOperation::None;
        };
        // AppKit proposes drops onto rows and between worktrees too; every
        // drop is retargeted to the gap between two cards.
        unsafe { outline.setDropItem_dropChildIndex(None, gap as NSInteger) };
        NSDragOperation::Move
    }

    fn accept_repo_drop(
        &self,
        outline: &NSOutlineView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
    ) -> bool {
        let Some((_, dragged, before)) = self.repo_drop(outline, info) else {
            return false;
        };
        let handler = LIST.with(|l| l.borrow().on_reorder.clone());
        match handler {
            Some(h) => {
                h.call((dragged, before));
                true
            }
            None => false,
        }
    }

    // MARK: Window construction

    fn build_window(&self, view: &MainWindow, mtm: MainThreadMarker) {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1000.0, 700.0)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&ns(&view.title));
        window.setMinSize(NSSize::new(view.min_size.0, view.min_size.1));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        // No `setFrameAutosaveName`: the frame is kept in the app's own state
        // file along with the scroll offset and the focused row, so that
        // `WTM_USER_DATA` sandboxes all of it together (see
        // `wtm_core::ui_state`).
        unsafe { window.setReleasedWhenClosed(false) };

        *self.ivars().toolbar.borrow_mut() = view.toolbar.clone();
        let toolbar = NSToolbar::initWithIdentifier(mtm.alloc(), &ns("wtm.toolbar"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(self)));
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        toolbar.setAllowsUserCustomization(false);
        window.setToolbar(Some(&toolbar));

        // Outline.
        let outline = OutlineView::new(NSRect::new(NSPoint::ZERO, NSSize::new(1000.0, 600.0)), mtm);
        let outline: Retained<NSOutlineView> = Retained::into_super(outline);
        let column = NSTableColumn::initWithIdentifier(mtm.alloc(), &ns("main"));
        column.setResizingMask(objc2_app_kit::NSTableColumnResizingOptions::AutoresizingMask);
        column.setWidth(960.0);
        column.setMinWidth(300.0);
        outline.addTableColumn(&column);
        unsafe { outline.setOutlineTableColumn(Some(&column)) };
        outline.setHeaderView(None);
        outline.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        // Rows draw their own card slices (see `rowview.rs`): no system
        // insets, spacing, selection or separators.
        outline.setStyle(NSTableViewStyle::Plain);
        // Rows draw the selection themselves, inside the card.
        outline.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::None);
        // Children are not indented: their plates are inset by `RowView`.
        outline.setIndentationPerLevel(0.0);
        // Otherwise the outline column grows by the deepest indentation and
        // rows end up wider than the clip view.
        outline.setAutoresizesOutlineColumn(false);
        outline.setIntercellSpacing(NSSize::new(0.0, 0.0));
        outline.setUsesAlternatingRowBackgroundColors(false);
        outline.setAllowsEmptySelection(true);
        outline.setFloatsGroupRows(false);
        outline.setBackgroundColor(&NSColor::clearColor());
        unsafe {
            outline.setDataSource(Some(ProtocolObject::from_ref(self)));
            outline.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        // Repos are reordered by dragging them within the list, and only
        // there: a repo dragged out of the window is not anything.
        outline.registerForDraggedTypes(&NSArray::from_retained_slice(&[ns(REPO_DRAG_TYPE)]));
        outline.setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        outline.setDraggingSourceOperationMask_forLocal(NSDragOperation::None, false);
        let scroll = NSScrollView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::ZERO, NSSize::new(1000.0, 600.0)),
        );
        outline.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
        scroll.setDocumentView(Some(&outline));
        // The column is sized to the clip view whenever that changes (see
        // `fit_column`); autoresizing alone left the table wider than the clip.
        let clip = scroll.contentView();
        clip.setPostsFrameChangedNotifications(true);
        // The clip view's bounds origin *is* the scroll offset, and it moves
        // for a wheel, a scroller, a keystroke and a programmatic scroll
        // alike, so one notification covers every way the list can move.
        clip.setPostsBoundsChangedNotifications(true);
        unsafe {
            let centre = objc2_foundation::NSNotificationCenter::defaultCenter();
            centre.addObserver_selector_name_object(
                self.as_ref(),
                sel!(clipFrameChanged:),
                Some(objc2_app_kit::NSViewFrameDidChangeNotification),
                Some(&clip),
            );
            centre.addObserver_selector_name_object(
                self.as_ref(),
                sel!(clipBoundsChanged:),
                Some(objc2_app_kit::NSViewBoundsDidChangeNotification),
                Some(&clip),
            );
        }
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);

        // Notice bar (hidden until there is something to say). Its content
        // is built from the view on each render (see `render_notice`).
        let notice_bar = NSView::new(mtm);
        notice_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        notice_bar.setHidden(true);
        // A notice must never resize the window: git output can run to
        // hundreds of lines.
        notice_bar
            .heightAnchor()
            .constraintLessThanOrEqualToConstant(64.0)
            .setActive(true);

        // Empty state, centred over the list.
        let empty_box = NSView::new(mtm);
        empty_box.setTranslatesAutoresizingMaskIntoConstraints(false);
        empty_box.setHidden(true);

        let spinner = NSProgressIndicator::new(mtm);
        spinner.setStyle(NSProgressIndicatorStyle::Spinning);
        spinner.setControlSize(NSControlSize::Small);
        spinner.setIndeterminate(true);
        spinner.setDisplayedWhenStopped(false);
        spinner.setTranslatesAutoresizingMaskIntoConstraints(false);

        let content = NSStackView::new(mtm);
        content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        content.setAlignment(NSLayoutAttribute::Width);
        content.setSpacing(0.0);
        content.setDistribution(NSStackViewDistribution::Fill);
        content.addArrangedSubview(&notice_bar);
        content.addArrangedSubview(&scroll);
        scroll.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityDefaultLow - 10.0,
            NSLayoutConstraintOrientation::Vertical,
        );
        content.addSubview(&empty_box);
        content.addSubview(&spinner);
        // Centred in the space the list would occupy, below the notice bar
        // rather than in the whole window, and never pushed up under the bar:
        // in a short window the centring gives way and the stack stays below
        // the bar, running off the bottom instead.
        let centred = empty_box
            .centerYAnchor()
            .constraintEqualToAnchor_constant(&scroll.centerYAnchor(), -20.0);
        centred.setPriority(NSLayoutPriorityDefaultHigh);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            // The stack's Width alignment does not stretch the bar on its own:
            // a notice shorter than the window left it hugging its text
            // against the right edge.
            notice_bar
                .widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor()),
            empty_box
                .centerXAnchor()
                .constraintEqualToAnchor(&content.centerXAnchor()),
            centred,
            empty_box
                .topAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&scroll.topAnchor(), 16.0),
            spinner
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&content.trailingAnchor(), -14.0),
            spinner
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&content.bottomAnchor(), -10.0),
        ]));
        window.setContentView(Some(&content));

        let iv = self.ivars();
        *iv.window.borrow_mut() = Some(window.clone());
        *iv.outline.borrow_mut() = Some(outline);
        *iv.column.borrow_mut() = Some(column);
        *iv.scroll.borrow_mut() = Some(scroll);
        *iv.notice_bar.borrow_mut() = Some(notice_bar);
        *iv.empty_box.borrow_mut() = Some(empty_box);
        *iv.activity.borrow_mut() = Some(spinner);

        // Where the program says: the frame it remembered, already checked
        // to land on a screen, else centred at the default size.
        match view.frame {
            Some(f) => window.setFrame_display(ns_rect(f), false),
            None => window.center(),
        }
        window.makeKeyAndOrderFront(None);
        self.fit_column();
    }

    // MARK: View → window

    pub fn render(&self, view: &View) {
        let mtm = MainThreadMarker::from(self);
        if self.window().is_none() {
            self.build_window(&view.window, mtm);
        }
        let iv = self.ivars();
        iv.rendering.set(true);
        crate::menu::render(&view.menus, self.as_ref(), mtm);
        self.render_toolbar(&view.window.toolbar);
        self.rebuild(&view.window.list);
        self.render_notice(view.window.notice.as_ref(), mtm);
        self.render_empty(view.window.empty.as_ref(), mtm);
        if let Some(o) = iv.outline.borrow().as_ref() {
            // Hide the list, not its scroll view. The content stack drops
            // hidden views from its layout, and with the scroll view gone a
            // notice bar would be the only thing in it, and the window would
            // shrink to the bar's height.
            o.setHidden(view.window.empty.is_some());
        }
        if let Some(sp) = iv.activity.borrow().as_ref() {
            if view.window.activity.spinning {
                unsafe { sp.startAnimation(None) };
            } else {
                unsafe { sp.stopAnimation(None) };
            }
            sp.setToolTip(view.window.activity.tooltip.as_deref().map(ns).as_deref());
        }
        iv.rendering.set(false);
        if let Some(window) = self.window() {
            crate::dialogs::render(&view.dialogs, &window, mtm);
        }
        crate::picker::render(view.popover.as_ref(), mtm);
        crate::settings::render(&view.panels, mtm);
    }

    fn render_toolbar(&self, items: &[ToolItem]) {
        *self.ivars().toolbar.borrow_mut() = items.to_vec();
        let Some(field) = self.ivars().search.borrow().clone() else {
            return;
        };
        let value = items.iter().find_map(|t| match t {
            ToolItem::Search { value, .. } => Some(value.clone()),
            _ => None,
        });
        // Only when it differs: setting it moves the insertion point to the
        // end, which mid-typing would be wrong.
        if let Some(v) = value {
            if field.stringValue().to_string() != v {
                field.setStringValue(&ns(&v));
            }
        }
    }

    fn render_notice(&self, notice: Option<&Element>, mtm: MainThreadMarker) {
        let Some(bar) = self.ivars().notice_bar.borrow().clone() else {
            return;
        };
        show_in(&bar, &self.ivars().notice, notice, mtm, |built, bar| {
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                built
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&bar.leadingAnchor(), 20.0),
                built
                    .trailingAnchor()
                    .constraintEqualToAnchor_constant(&bar.trailingAnchor(), -12.0),
                built
                    .topAnchor()
                    .constraintEqualToAnchor_constant(&bar.topAnchor(), 8.0),
                built
                    .bottomAnchor()
                    .constraintEqualToAnchor_constant(&bar.bottomAnchor(), -8.0),
            ]));
        });
    }

    fn render_empty(&self, empty: Option<&Element>, mtm: MainThreadMarker) {
        let Some(holder) = self.ivars().empty_box.borrow().clone() else {
            return;
        };
        show_in(&holder, &self.ivars().empty, empty, mtm, |built, holder| {
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                built
                    .leadingAnchor()
                    .constraintEqualToAnchor(&holder.leadingAnchor()),
                built
                    .trailingAnchor()
                    .constraintEqualToAnchor(&holder.trailingAnchor()),
                built
                    .topAnchor()
                    .constraintEqualToAnchor(&holder.topAnchor()),
                built
                    .bottomAnchor()
                    .constraintEqualToAnchor(&holder.bottomAnchor()),
            ]));
        });
    }

    // MARK: The list

    /// Bring the outline in line with `list`, applying the smallest update
    /// that covers the difference from what is on screen.
    fn rebuild(&self, list: &TreeList) {
        let mtm = MainThreadMarker::from(self);
        let iv = self.ivars();

        let mut items = iv.items.borrow_mut();
        let mut item_for = |kind: ItemKind, key: &str, section: &str| -> Retained<WTMItem> {
            let item = items
                .entry(key.to_string())
                .or_insert_with(|| WTMItem::new(kind, key, section, mtm));
            // A key is a row's identity, but not its kind: a pending row and
            // a worktree never share one, so this only guards a program bug.
            if item.kind() != kind || item.section() != section {
                *item = WTMItem::new(kind, key, section, mtm);
            }
            item.clone()
        };
        let mut tree = Tree::default();
        for section in &list.sections {
            let rows = section
                .rows
                .iter()
                .map(|r| {
                    let kind = match r.content {
                        RowContent::Worktree(_) => ItemKind::Worktree,
                        RowContent::Pending(_) => ItemKind::Pending,
                    };
                    item_for(kind, &r.key, &section.key)
                })
                .collect();
            tree.children.insert(section.key.clone(), rows);
            tree.roots
                .push(item_for(ItemKind::Header, &section.key, &section.key));
        }
        // Items for rows that are gone: an outline that still holds one
        // compares it by identity, so it is never handed out again.
        let live: HashSet<&str> = tree
            .roots
            .iter()
            .chain(tree.children.values().flatten())
            .map(|i| i.key())
            .collect();
        let live: HashSet<String> = live.into_iter().map(str::to_string).collect();
        items.retain(|k, _| live.contains(k));
        drop(items);

        let Some(outline) = iv.outline.borrow().clone() else {
            *iv.tree.borrow_mut() = tree;
            LIST.with(|l| *l.borrow_mut() = list.clone());
            return;
        };

        // Planned while the old tree is still the data source: the outline
        // may read from it to answer, and must answer about the old rows.
        let first = iv.tree.borrow().roots.is_empty() && outline.numberOfRows() == 0;
        let plan = plan_splice(&outline, &iv.tree.borrow(), &tree);
        let previous = LIST.with(|l| std::mem::replace(&mut *l.borrow_mut(), list.clone()));
        *iv.tree.borrow_mut() = tree;
        // What the user sees closed, as the view has it; the outline is
        // brought in line below.
        *iv.collapsed.borrow_mut() = list
            .sections
            .iter()
            .filter(|s| !s.expanded)
            .map(|s| s.key.clone())
            .collect();

        let roots = iv.tree.borrow().roots.clone();
        let Some(Plan {
            roots: root_change,
            cards,
        }) = plan.filter(|_| !first)
        else {
            // The first tree, reordered worktree rows, repos reordered as
            // others came or went, or an outline that no longer shows the
            // tree it was last told about: start over.
            outline.reloadData();
            self.open_or_close(&outline, list, &roots);
            self.sync_row_styles();
            self.sync_selection(&outline, list);
            #[cfg(debug_assertions)]
            self.check_outline(&outline);
            return;
        };

        // Rows that stay keep their views: rebuilding every visible row costs
        // tens of milliseconds, more than a key repeat allows while a short
        // query matches most of the list.
        let mut resized = Vec::new();
        NSAnimationContext::beginGrouping();
        NSAnimationContext::currentContext().setDuration(0.0);
        outline.beginUpdates();
        match &root_change {
            Roots::Spliced(s) => apply_splice(&outline, None, s),
            // A repo dragged or moved to a new place. An open card's rows go
            // with its header, and the selection with its row.
            Roots::Moved(moves) => {
                for &(from, to) in moves {
                    unsafe {
                        outline.moveItemAtIndex_inParent_toIndex_inParent(
                            from as NSInteger,
                            None,
                            to as NSInteger,
                            None,
                        )
                    };
                }
            }
        }
        for card in &cards {
            let CardRows::Spliced(s) = &card.rows else {
                continue;
            };
            apply_splice(&outline, Some(&card.root), s);
            // A card's first and last rows are taller (the well's padding),
            // and the outline keeps the height of a row that stays.
            let new_ids = identities(&card.new);
            for end in [
                card.old.first(),
                card.old.last(),
                card.new.first(),
                card.new.last(),
            ]
            .into_iter()
            .flatten()
            {
                if new_ids.contains(&Retained::as_ptr(end)) {
                    resized.push(end.clone());
                }
            }
        }
        outline.endUpdates();
        NSAnimationContext::endGrouping();

        for card in &cards {
            if matches!(card.rows, CardRows::Reloaded) {
                unsafe { outline.reloadItem_reloadChildren(Some(&card.root), true) };
            }
        }
        self.open_or_close(&outline, list, &roots);
        // A card moved from the keyboard stays in view.
        if matches!(root_change, Roots::Moved(_)) {
            let row = outline.selectedRow();
            if row >= 0 {
                outline.scrollRowToVisible(row);
            }
        }

        // What stayed: reload the rows whose content differs.
        for card in &cards {
            if matches!(card.rows, CardRows::Reloaded) {
                continue;
            }
            let key = card.root.key();
            let (old, new) = (previous.section(key), list.section(key));
            if old.map(|s| &s.header) != new.map(|s| &s.header) {
                unsafe { outline.reloadItem(Some(&card.root)) };
            }
            let old_ids = identities(&card.old);
            for child in &card.new {
                // A row just inserted was configured from this list.
                if !old_ids.contains(&Retained::as_ptr(child)) {
                    continue;
                }
                let k = child.key();
                if previous.row(k).map(|r| &r.content) != list.row(k).map(|r| &r.content) {
                    unsafe { outline.reloadItem(Some(child)) };
                }
            }
        }
        self.sync_row_styles_noting(&resized);
        self.sync_selection(&outline, list);
        #[cfg(debug_assertions)]
        self.check_outline(&outline);
    }

    /// Open the cards the view has open and close the rest. The outline
    /// keeps an item's expansion across `reloadData`, so a card is closed
    /// here as well as opened.
    fn open_or_close(&self, outline: &NSOutlineView, list: &TreeList, roots: &[Retained<WTMItem>]) {
        for (root, section) in roots.iter().zip(&list.sections) {
            let open = unsafe { outline.isItemExpanded(Some(root)) };
            if section.expanded && !open {
                unsafe { outline.expandItem(Some(root)) };
            } else if !section.expanded && open {
                unsafe { outline.collapseItem(Some(root)) };
            }
        }
    }

    /// Select the row the view has selected, if it is not already.
    fn sync_selection(&self, outline: &NSOutlineView, list: &TreeList) {
        let want = list
            .selected
            .as_deref()
            .and_then(|k| self.row_of(outline, k))
            .map(|(_, row)| row);
        let have = Some(outline.selectedRow()).filter(|&r| r >= 0);
        if want == have {
            return;
        }
        match want {
            Some(row) => outline.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            ),
            None => unsafe { outline.deselectAll(None) },
        }
    }

    /// Debug builds check after every rebuild that the outline shows exactly
    /// the tree, at the heights the delegate gives: a wrong splice would
    /// otherwise surface as an AppKit exception much later, or as rows drawn
    /// at the wrong height.
    #[cfg(debug_assertions)]
    fn check_outline(&self, outline: &NSOutlineView) {
        let expected = tree_rows(outline, &self.ivars().tree.borrow());
        assert_eq!(shown_rows(outline), identities(&expected), "outline rows");
        for (row, item) in (0..).zip(&expected) {
            let (height, want) = (
                outline.rectOfRow(row).size.height,
                self.row_height(outline, item),
            );
            assert!(
                (height - want).abs() < 0.01,
                "row {row} ({}) is {height} high, not {want}",
                item.key()
            );
        }
    }

    // MARK: Effects

    pub fn perform(&self, effect: Effect) {
        let mtm = MainThreadMarker::from(self);
        match effect {
            Effect::After(delay, then) => {
                let then = MainThreadBound::new(then, mtm);
                let ns = i64::try_from(delay.as_nanos()).unwrap_or(i64::MAX);
                let _ =
                    DispatchQueue::main().after(dispatch2::DispatchTime::NOW.time(ns), move || {
                        let mtm = MainThreadMarker::new().expect("main queue");
                        then.get(mtm).call(());
                    });
            }
            Effect::Copy(text) => copy_to_pasteboard(&text),
            Effect::PickFolders(pick) => {
                let window = match &pick.parent {
                    Some(key) => crate::settings::window(key, mtm),
                    None => self.window(),
                };
                if let Some(window) = window {
                    crate::dialogs::pick_folders(
                        &window,
                        &pick.title,
                        pick.multiple,
                        move |paths| pick.on_done.call(paths),
                    );
                } else {
                    pick.on_done.call(Vec::new());
                }
            }
            Effect::FocusSearch => {
                if let (Some(w), Some(s)) = (self.window(), self.ivars().search.borrow().as_ref()) {
                    w.makeFirstResponder(Some(s));
                }
            }
            Effect::FocusList => focus_tree(mtm),
            // Once the outline has laid out this render's rows: an offset
            // clamped against a one-row-tall outline would come out at zero.
            Effect::ScrollTo(offset) => on_next_turn(move |c| c.scroll_to(offset)),
            Effect::RevealSelection => {
                if let Some(o) = self.ivars().outline.borrow().as_ref() {
                    let row = o.selectedRow();
                    if row >= 0 {
                        o.scrollRowToVisible(row);
                    }
                }
            }
            Effect::PresentPanel(key) => crate::settings::present(&key, mtm),
        }
    }

    /// Clamped, because the list may be shorter than it was: worktrees
    /// deleted elsewhere, or a card closed.
    fn scroll_to(&self, offset: f64) {
        let Some(scroll) = self.ivars().scroll.borrow().clone() else {
            return;
        };
        let clip = scroll.contentView();
        let document = scroll.documentView().map_or(0.0, |d| d.frame().size.height);
        let y = offset.clamp(0.0, (document - clip.bounds().size.height).max(0.0));
        clip.scrollToPoint(NSPoint::new(clip.bounds().origin.x, y));
        scroll.reflectScrolledClipView(&clip);
    }
}

/// Show `element` in `holder`, patching what is there when it has the same
/// shape and building it afresh when not; hide `holder` for `None`. `pin`
/// places a newly built view in the holder.
fn show_in(
    holder: &NSView,
    shown: &RefCell<Option<Shown>>,
    element: Option<&Element>,
    mtm: MainThreadMarker,
    pin: impl Fn(&NSView, &NSView),
) {
    let Some(element) = element else {
        holder.setHidden(true);
        return;
    };
    holder.setHidden(false);
    let mut shown = shown.borrow_mut();
    if let Some(s) = shown.as_mut() {
        if same_shape(&s.element, element) {
            s.built.patch(&s.element, element);
            s.element = element.clone();
            return;
        }
        s.built.view.removeFromSuperview();
    }
    let built = Built::new(element, mtm);
    built
        .view
        .setTranslatesAutoresizingMaskIntoConstraints(false);
    holder.addSubview(&built.view);
    pin(&built.view, holder);
    *shown = Some(Shown {
        element: element.clone(),
        built,
    });
}

/// How to take the outline from `old` to `new` in place.
struct Plan {
    roots: Roots,
    /// Each repo that stays, with how its rows change.
    cards: Vec<Card>,
}

/// How the list of repos changes. A reordering that also adds or removes
/// repos is neither, and is left to `reloadData`.
enum Roots {
    /// The repos that come and go.
    Spliced(Splice),
    /// The same repos in a new order.
    Moved(Vec<(usize, usize)>),
}

/// How to take the outline from `old` to `new` with moves, insertions and
/// removals. Repos are either moved or inserted and removed, never both at
/// once, and worktrees never move. `None` when that cannot be done safely —
/// worktree rows reordered, repos reordered as others come or go, or the
/// outline not showing `old` — since a splice naming rows the outline does
/// not have raises an AppKit exception.
fn plan_splice(outline: &NSOutlineView, old: &Tree, new: &Tree) -> Option<Plan> {
    if shown_rows(outline) != identities(&tree_rows(outline, old)) {
        return None;
    }
    let (old_roots, new_roots) = (identities(&old.roots), identities(&new.roots));
    let roots = match splice(&old_roots, &new_roots) {
        Some(s) => Roots::Spliced(s),
        None => Roots::Moved(moves(&old_roots, &new_roots)?),
    };
    let mut cards = Vec::new();
    for (i, root) in new.roots.iter().enumerate() {
        if matches!(&roots, Roots::Spliced(s) if s.inserted.contains(&i)) {
            continue;
        }
        let key = root.key().to_string();
        let old = old.children.get(&key).cloned().unwrap_or_default();
        let new = new.children.get(&key).cloned().unwrap_or_default();
        let (old_ids, new_ids) = (identities(&old), identities(&new));
        let rows = if old_ids == new_ids {
            CardRows::Same
        } else if unsafe { outline.isItemExpanded(Some(root)) } {
            CardRows::Spliced(splice(&old_ids, &new_ids)?)
        } else {
            CardRows::Reloaded
        };
        cards.push(Card {
            root: root.clone(),
            old,
            new,
            rows,
        });
    }
    Some(Plan { roots, cards })
}

/// The rows `tree` makes with the cards open or closed as the outline has
/// them now.
fn tree_rows(outline: &NSOutlineView, tree: &Tree) -> Vec<Retained<WTMItem>> {
    let mut rows = Vec::new();
    for root in &tree.roots {
        rows.push(root.clone());
        if unsafe { outline.isItemExpanded(Some(root)) } {
            rows.extend(
                tree.children
                    .get(&root.key().to_string())
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
    }
    rows
}

/// The items of the outline's rows, as it holds them.
fn shown_rows(outline: &NSOutlineView) -> Vec<*const WTMItem> {
    (0..outline.numberOfRows())
        .map(|row| {
            outline
                .itemAtRow(row)
                .map_or(std::ptr::null(), |i| Retained::as_ptr(&i).cast())
        })
        .collect()
}

/// A repo shown before and after an update.
struct Card {
    root: Retained<WTMItem>,
    /// Its rows as the outline last showed them.
    old: Vec<Retained<WTMItem>>,
    new: Vec<Retained<WTMItem>>,
    rows: CardRows,
}

/// How a card's rows change in an update.
enum CardRows {
    Same,
    /// Rows inserted and removed around the ones that stay.
    Spliced(Splice),
    /// Read again from the tree: the card is closed, so none are on screen.
    Reloaded,
}

/// Tell the outline which of `parent`'s children (the repos, for `None`)
/// went and which arrived.
fn apply_splice(outline: &NSOutlineView, parent: Option<&AnyObject>, s: &Splice) {
    unsafe {
        outline.removeItemsAtIndexes_inParent_withAnimation(
            &index_set(&s.removed),
            parent,
            NSTableViewAnimationOptions::EffectNone,
        );
        outline.insertItemsAtIndexes_inParent_withAnimation(
            &index_set(&s.inserted),
            parent,
            NSTableViewAnimationOptions::EffectNone,
        );
    }
}

/// What a drag of `item` carries: a section's key, or nothing for any other row,
/// which cannot be dragged.
fn repo_drag_writer(item: &AnyObject) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
    let item = item.downcast_ref::<WTMItem>()?;
    if item.kind() != ItemKind::Header || LIST.with(|l| l.borrow().on_reorder.is_none()) {
        return None;
    }
    let writer = NSPasteboardItem::new();
    writer.setString_forType(&ns(item.key()), &ns(REPO_DRAG_TYPE));
    Some(ProtocolObject::from_retained(writer))
}

/// Items by identity, which is how the outline tells rows apart.
fn identities(items: &[Retained<WTMItem>]) -> Vec<*const WTMItem> {
    items.iter().map(Retained::as_ptr).collect()
}

fn index_set(indices: &[usize]) -> Retained<NSIndexSet> {
    let set = NSMutableIndexSet::new();
    for &i in indices {
        set.addIndex(i);
    }
    Retained::into_super(set)
}

/// The frame versions before `ui-state.json` had AppKit autosave, for the
/// first launch after the update. They restored its size (their
/// `window.center()` replaced only the position), so without it that launch
/// would open at the default size.
pub fn legacy_autosaved_frame() -> Option<WindowFrame> {
    let saved = NSUserDefaults::standardUserDefaults().stringForKey(&ns(LEGACY_FRAME_KEY))?;
    parse_autosaved_frame(&saved.to_string())
}

/// AppKit's frame string is the window's frame, `x y width height` in screen
/// coordinates, followed by the frame of the screen it was on.
fn parse_autosaved_frame(saved: &str) -> Option<WindowFrame> {
    let mut numbers = saved.split_whitespace().map(|n| n.parse::<f64>());
    let mut next = || numbers.next()?.ok();
    Some(WindowFrame {
        x: next()?,
        y: next()?,
        width: next()?,
        height: next()?,
    })
}

fn frame(r: NSRect) -> Frame {
    Frame {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

fn ns_rect(f: Frame) -> NSRect {
    NSRect::new(NSPoint::new(f.x, f.y), NSSize::new(f.width, f.height))
}

/// Run `f` on the controller on the next turn of the main run loop.
fn on_next_turn(f: impl FnOnce(&Controller) + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("main queue");
        if let Some(c) = controller(mtm) {
            f(c);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::parse_autosaved_frame;
    use wtm_core::WindowFrame;

    #[test]
    fn an_autosaved_frame_is_read_up_to_the_screen() {
        assert_eq!(
            parse_autosaved_frame("256 187 1000 732 0 0 1512 949 "),
            Some(WindowFrame {
                x: 256.0,
                y: 187.0,
                width: 1000.0,
                height: 732.0,
            })
        );
        assert_eq!(parse_autosaved_frame(""), None);
        assert_eq!(parse_autosaved_frame("256 187 1000"), None);
        assert_eq!(parse_autosaved_frame("256 187 wide 732"), None);
    }
}
