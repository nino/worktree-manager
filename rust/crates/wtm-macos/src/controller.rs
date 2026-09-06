//! The single window controller: owns the outline view, translates model
//! snapshots into targeted row reloads, and routes toolbar/menu actions.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBackingStoreType, NSBezelStyle, NSButton, NSColor,
    NSControl, NSControlSize, NSControlTextEditingDelegate, NSFont, NSLayoutAttribute,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSMenuItem,
    NSMenuItemValidation, NSOutlineView, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSProgressIndicator, NSProgressIndicatorStyle, NSScrollView, NSSearchField,
    NSSearchFieldDelegate, NSSearchToolbarItem, NSStackView, NSStackViewDistribution,
    NSTableColumn, NSTableViewColumnAutoresizingStyle, NSTableViewSelectionHighlightStyle,
    NSTableViewStyle, NSTextField, NSTextFieldDelegate, NSToolbar, NSToolbarDelegate,
    NSToolbarDisplayMode, NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem,
    NSToolbarItemIdentifier, NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWindowToolbarStyle,
};
use objc2_foundation::{
    NSArray, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSURL,
};
use wtm_core::model::Tone;
use wtm_core::{Action, App, Event, Model};

use crate::cells::{
    PendingCell, RepoCell, WorktreeCell, PENDING_ROW_HEIGHT, REPO_ROW_HEIGHT, WORKTREE_ROW_HEIGHT,
};
use crate::dialogs;
use crate::items::{ItemKind, WTMItem};
use crate::outline::OutlineView;
use crate::rowview::{RowStyle, RowView, LAST_ROW_EXTRA};
use crate::util::{ns, secondary_label, symbol};

static CONTROLLER: OnceLock<MainThreadBound<Retained<Controller>>> = OnceLock::new();
/// A model-changed notification is already queued for the main thread.
static REPAINT_QUEUED: AtomicBool = AtomicBool::new(false);

pub fn controller(mtm: MainThreadMarker) -> Option<&'static Retained<Controller>> {
    CONTROLLER.get().map(|c| c.get(mtm))
}

pub fn main_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    controller(mtm).and_then(|c| c.ivars().window.borrow().clone())
}

const TOOLBAR_ADD: &str = "wtm.add";
const TOOLBAR_REFRESH: &str = "wtm.refresh";
const TOOLBAR_SEARCH: &str = "wtm.search";
const TOOLBAR_SETTINGS: &str = "wtm.settings";

/// The tree the outline currently shows: repo items in order, each with its
/// visible children (filtered worktrees then pending creations, sorted the
/// way git will list them).
#[derive(Default)]
struct Tree {
    roots: Vec<Retained<WTMItem>>,
    children: HashMap<String, Vec<Retained<WTMItem>>>,
}

pub struct ControllerIvars {
    app: App,
    window: RefCell<Option<Retained<NSWindow>>>,
    outline: RefCell<Option<Retained<NSOutlineView>>>,
    column: RefCell<Option<Retained<NSTableColumn>>>,
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    search: RefCell<Option<Retained<NSSearchField>>>,
    empty: RefCell<Option<Retained<NSView>>>,
    notice_bar: RefCell<Option<Retained<NSView>>>,
    notice_label: RefCell<Option<Retained<NSTextField>>>,
    notice_details: RefCell<Option<Retained<NSButton>>>,
    refresh_spinner: RefCell<Option<Retained<NSProgressIndicator>>>,
    items: RefCell<HashMap<String, Retained<WTMItem>>>,
    tree: RefCell<Tree>,
    /// The model the tree was last built from.
    shown: RefCell<Option<Arc<Model>>>,
    query: RefCell<String>,
    collapsed: RefCell<HashSet<String>>,
    /// Id of the notice currently displayed, for the auto-clear timer.
    notice_id: Cell<u64>,
    /// When the app last became active; `None` until launch has settled.
    last_activation: Cell<Option<std::time::Instant>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMController"]
    #[ivars = ControllerIvars]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    // MARK: Actions (toolbar + menu)

    impl Controller {
        #[unsafe(method(addRepo:))]
        fn add_repo(&self, _s: Option<&AnyObject>) {
            if let Some(w) = self.window() {
                dialogs::add_repos(&self.ivars().app, &w);
            }
        }

        #[unsafe(method(refresh:))]
        fn refresh(&self, _s: Option<&AnyObject>) {
            self.ivars().app.dispatch(Action::RefreshAll);
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _s: Option<&AnyObject>) {
            if let Some(w) = self.window() {
                dialogs::settings(&self.ivars().app, &w);
            }
        }

        #[unsafe(method(newWorktree:))]
        fn new_worktree(&self, _s: Option<&AnyObject>) {
            let Some(w) = self.window() else { return };
            if let Some(repo_id) = self.selected_repo_id() {
                dialogs::create_worktree(&self.ivars().app, &w, &repo_id);
            }
        }

        #[unsafe(method(focusSearch:))]
        fn focus_search(&self, _s: Option<&AnyObject>) {
            if let (Some(w), Some(s)) = (self.window(), self.ivars().search.borrow().as_ref()) {
                w.makeFirstResponder(Some(s));
            }
        }

        #[unsafe(method(dismissNotice:))]
        fn dismiss_notice(&self, _s: Option<&AnyObject>) {
            self.ivars().app.dispatch(Action::ClearNotice);
        }

        /// Show the current notice in full, in a scrolling sheet.
        #[unsafe(method(showNoticeDetails:))]
        fn show_notice_details(&self, _s: Option<&AnyObject>) {
            let model = self.ivars().app.model();
            if let (Some(w), Some(n)) = (self.window(), model.notice.as_ref()) {
                let title = n.text.lines().next().unwrap_or("Details").trim_end_matches(':');
                dialogs::text_sheet(&w, title, &n.text);
            }
        }

        /// The clip view changed size: make the single column exactly that
        /// wide, so rows never extend past the visible area.
        #[unsafe(method(clipFrameChanged:))]
        fn clip_frame_changed(&self, _n: &NSNotification) {
            self.fit_column();
        }
    }

    unsafe impl NSMenuItemValidation for Controller {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            if item.action() == Some(sel!(newWorktree:)) {
                self.selected_repo_id().is_some()
            } else {
                true
            }
        }
    }

    // MARK: Application lifecycle

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = MainThreadMarker::from(self);
            self.build_window(mtm);
            self.model_changed();
            self.ivars().app.start();
            self.ivars().last_activation.set(Some(std::time::Instant::now()));
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _n: &NSNotification) {
            // Coming back to the app is the moment stale status would be
            // noticed; a refresh is cheap and runs off the main thread. The
            // launch activation is skipped (start() already lists), as are
            // rapid re-activations (Cmd-Tab flicker).
            let iv = self.ivars();
            let now = std::time::Instant::now();
            let recent = iv
                .last_activation
                .get()
                .map(|t| now.duration_since(t) < std::time::Duration::from_secs(2))
                .unwrap_or(true);
            iv.last_activation.set(Some(now));
            if !recent {
                iv.app.dispatch(Action::RefreshAll);
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _a: &NSApplication) -> bool {
            true
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
                self.ivars().app.dispatch(Action::AddRepos(paths));
            }
        }
    }

    unsafe impl NSWindowDelegate for Controller {}

    // MARK: Toolbar

    unsafe impl NSToolbarDelegate for Controller {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(&self, _t: &NSToolbar, ident: &NSToolbarItemIdentifier, _flag: bool) -> Option<Retained<NSToolbarItem>> {
            self.make_toolbar_item(ident)
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn default_items(&self, _t: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            toolbar_identifiers()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed_items(&self, _t: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            toolbar_identifiers()
        }
    }

    // MARK: Search

    unsafe impl NSControlTextEditingDelegate for Controller {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, n: &NSNotification) {
            let Some(obj) = n.object() else { return };
            let Some(field) = obj.downcast_ref::<NSControl>() else { return };
            let q = field.stringValue().to_string();
            if *self.ivars().query.borrow() != q {
                *self.ivars().query.borrow_mut() = q;
                self.rebuild(true);
            }
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
                Some(i) => tree.children.get(&i.kind().key()).map(|c| c.len()).unwrap_or(0) as NSInteger,
            }
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        unsafe fn child(&self, _o: &NSOutlineView, index: NSInteger, item: Option<&AnyObject>) -> Retained<AnyObject> {
            let tree = self.ivars().tree.borrow();
            let child = match item.and_then(|i| i.downcast_ref::<WTMItem>()) {
                None => tree.roots[index as usize].clone(),
                Some(i) => tree.children[&i.kind().key()][index as usize].clone(),
            };
            Retained::into_super(Retained::into_super(child))
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        unsafe fn is_expandable(&self, _o: &NSOutlineView, item: &AnyObject) -> bool {
            matches!(item.downcast_ref::<WTMItem>().map(|i| i.kind()), Some(ItemKind::Repo { .. }))
        }
    }

    // MARK: Outline delegate

    unsafe impl NSOutlineViewDelegate for Controller {
        #[unsafe(method_id(outlineView:viewForTableColumn:item:))]
        unsafe fn view_for_item(&self, outline: &NSOutlineView, _c: Option<&NSTableColumn>, item: &AnyObject) -> Option<Retained<NSView>> {
            self.make_cell_view(outline, item)
        }

        #[unsafe(method(outlineView:heightOfRowByItem:))]
        unsafe fn height_of_row(&self, _o: &NSOutlineView, item: &AnyObject) -> f64 {
            let Some(item) = item.downcast_ref::<WTMItem>() else {
                return PENDING_ROW_HEIGHT;
            };
            let base = match item.kind() {
                ItemKind::Repo { .. } => REPO_ROW_HEIGHT,
                ItemKind::Worktree { .. } => WORKTREE_ROW_HEIGHT,
                ItemKind::Pending { .. } => PENDING_ROW_HEIGHT,
            };
            let style = self.row_style(_o, item);
            match style {
                RowStyle::Child { last: true, .. } => base + LAST_ROW_EXTRA,
                _ => base,
            }
        }

        #[unsafe(method(outlineView:shouldSelectItem:))]
        unsafe fn should_select(&self, _o: &NSOutlineView, _item: &AnyObject) -> bool {
            // Nothing acts on a selection; every control lives in the rows.
            false
        }

        #[unsafe(method_id(outlineView:rowViewForItem:))]
        unsafe fn row_view_for_item(&self, outline: &NSOutlineView, item: &AnyObject) -> Option<Retained<objc2_app_kit::NSTableRowView>> {
            self.make_row_view(outline, item)
        }

        #[unsafe(method(outlineViewItemDidExpand:))]
        fn did_expand(&self, n: &NSNotification) {
            if let Some(id) = expanded_repo_id(n) {
                self.ivars().collapsed.borrow_mut().remove(&id);
            }
            self.sync_row_styles_later();
        }

        #[unsafe(method(outlineViewItemDidCollapse:))]
        fn did_collapse(&self, n: &NSNotification) {
            if let Some(id) = expanded_repo_id(n) {
                self.ivars().collapsed.borrow_mut().insert(id);
            }
            self.sync_row_styles_later();
        }
    }
);

/// The repo id carried by an expand/collapse notification.
fn expanded_repo_id(n: &NSNotification) -> Option<String> {
    let info = n.userInfo()?;
    let obj = info.objectForKey(&*ns("NSObject"))?;
    let item = obj.downcast_ref::<WTMItem>()?;
    match item.kind() {
        ItemKind::Repo { repo_id } => Some(repo_id),
        _ => None,
    }
}

fn toolbar_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    let flexible = unsafe { NSToolbarFlexibleSpaceItemIdentifier }.to_string();
    NSArray::from_retained_slice(&[
        ns(TOOLBAR_ADD),
        ns(TOOLBAR_REFRESH),
        ns(&flexible),
        ns(TOOLBAR_SEARCH),
        ns(TOOLBAR_SETTINGS),
    ])
}

impl Controller {
    fn make_toolbar_item(
        &self,
        ident: &NSToolbarItemIdentifier,
    ) -> Option<Retained<NSToolbarItem>> {
        let mtm = MainThreadMarker::from(self);
        let id = ident.to_string();
        let make = |label: &str, sym: &str, action: objc2::runtime::Sel| {
            let item = NSToolbarItem::initWithItemIdentifier(mtm.alloc(), ident);
            item.setLabel(&ns(label));
            item.setToolTip(Some(&ns(label)));
            item.setImage(symbol(sym, label).as_deref());
            item.setBordered(true);
            unsafe {
                item.setTarget(Some(self.as_ref()));
                item.setAction(Some(action));
            }
            item
        };
        match id.as_str() {
            TOOLBAR_ADD => Some(make("Add Repo", "plus", sel!(addRepo:))),
            TOOLBAR_REFRESH => Some(make("Refresh", "arrow.clockwise", sel!(refresh:))),
            TOOLBAR_SETTINGS => Some(make("Settings", "gearshape", sel!(openSettings:))),
            TOOLBAR_SEARCH => {
                let item = NSSearchToolbarItem::initWithItemIdentifier(mtm.alloc(), ident);
                item.setPreferredWidthForSearchField(220.0);
                let field = item.searchField();
                field.setPlaceholderString(Some(&ns("Search worktrees")));
                field.setSendsSearchStringImmediately(true);
                field.setSendsWholeSearchString(false);
                unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
                *self.ivars().search.borrow_mut() = Some(field);
                Some(Retained::into_super(item))
            }
            _ => None,
        }
    }

    /// The card slice a row should draw, from the display tree and expansion.
    fn row_style(&self, outline: &NSOutlineView, item: &WTMItem) -> RowStyle {
        let tree = self.ivars().tree.borrow();
        match item.kind() {
            ItemKind::Repo { .. } => {
                let has_children = tree
                    .children
                    .get(&item.kind().key())
                    .map(|c| !c.is_empty())
                    .unwrap_or(false);
                let expanded = unsafe { outline.isItemExpanded(Some(item)) };
                RowStyle::Header {
                    closed: !(has_children && expanded),
                }
            }
            ItemKind::Worktree { repo_id, .. } | ItemKind::Pending { repo_id, .. } => {
                let key = ItemKind::Repo { repo_id }.key();
                let children = tree.children.get(&key);
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
        view.set_style(style);
        Some(Retained::into_super(view))
    }

    /// Expand/collapse notifications arrive mid-operation, where the outline
    /// forbids re-entrant height changes; sync on the next run-loop turn.
    fn sync_row_styles_later(&self) {
        DispatchQueue::main().exec_async(|| {
            let mtm = MainThreadMarker::new().expect("main queue");
            if let Some(c) = controller(mtm) {
                c.sync_row_styles();
            }
        });
    }

    /// Row views are reused by the outline, so after any structural change
    /// (rows added/removed, expand/collapse) each visible row is told which
    /// card slice it now is: a former last child becomes a middle one, a
    /// collapsed header closes its card.
    fn sync_row_styles(&self) {
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
        }
        // A row that became (or stopped being) its card's last one changed
        // height; a zero-duration group keeps the relayout instant.
        if heights_changed {
            let all = objc2_foundation::NSIndexSet::indexSetWithIndexesInRange(
                objc2_foundation::NSRange::new(0, outline.numberOfRows() as usize),
            );
            objc2_app_kit::NSAnimationContext::beginGrouping();
            objc2_app_kit::NSAnimationContext::currentContext().setDuration(0.0);
            outline.noteHeightOfRowsWithIndexesChanged(&all);
            objc2_app_kit::NSAnimationContext::endGrouping();
        }
    }

    fn make_cell_view(
        &self,
        outline: &NSOutlineView,
        item: &AnyObject,
    ) -> Option<Retained<NSView>> {
        let mtm = MainThreadMarker::from(self);
        let item = item.downcast_ref::<WTMItem>()?;
        let shown = self.ivars().shown.borrow();
        let model = shown.as_ref()?;
        let app = &self.ivars().app;
        match item.kind() {
            ItemKind::Repo { repo_id } => {
                let node = model.repo(&repo_id)?;
                let cell = match unsafe {
                    outline.makeViewWithIdentifier_owner(&ns(RepoCell::IDENTIFIER), None)
                } {
                    Some(v) => v.downcast::<RepoCell>().ok()?,
                    None => RepoCell::new(app.clone(), mtm),
                };
                let visible = self
                    .ivars()
                    .tree
                    .borrow()
                    .children
                    .get(&item.kind().key())
                    .map(|c| c.len())
                    .unwrap_or(0);
                cell.configure(
                    node,
                    model,
                    visible,
                    !self.ivars().query.borrow().trim().is_empty(),
                );
                Some(Retained::into_super(Retained::into_super(cell)))
            }
            ItemKind::Worktree { repo_id, path } => {
                let node = model.repo(&repo_id)?;
                let w = node.worktree(&path)?;
                let cell = match unsafe {
                    outline.makeViewWithIdentifier_owner(&ns(WorktreeCell::IDENTIFIER), None)
                } {
                    Some(v) => v.downcast::<WorktreeCell>().ok()?,
                    None => WorktreeCell::new(app.clone(), mtm),
                };
                cell.configure(
                    &node.repo,
                    w,
                    &node.branches,
                    model.busy_for(&path),
                    &model.home,
                );
                Some(Retained::into_super(Retained::into_super(cell)))
            }
            ItemKind::Pending { id, .. } => {
                let p = model.pending.iter().find(|p| p.id == id)?;
                let cell = match unsafe {
                    outline.makeViewWithIdentifier_owner(&ns(PendingCell::IDENTIFIER), None)
                } {
                    Some(v) => v.downcast::<PendingCell>().ok()?,
                    None => PendingCell::new(app.clone(), mtm),
                };
                cell.configure(p);
                Some(Retained::into_super(Retained::into_super(cell)))
            }
        }
    }

    pub fn new(app: App, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ControllerIvars {
            app,
            window: RefCell::new(None),
            outline: RefCell::new(None),
            column: RefCell::new(None),
            scroll: RefCell::new(None),
            search: RefCell::new(None),
            empty: RefCell::new(None),
            notice_bar: RefCell::new(None),
            notice_label: RefCell::new(None),
            notice_details: RefCell::new(None),
            refresh_spinner: RefCell::new(None),
            items: RefCell::new(HashMap::new()),
            tree: RefCell::new(Tree::default()),
            shown: RefCell::new(None),
            query: RefCell::new(String::new()),
            collapsed: RefCell::new(HashSet::new()),
            notice_id: Cell::new(0),
            last_activation: Cell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let _ = CONTROLLER.set(MainThreadBound::new(this.clone(), mtm));

        // Model changes arrive on core threads; coalesce and hop to main.
        this.ivars().app.subscribe(|_: Event| {
            if !REPAINT_QUEUED.swap(true, Ordering::AcqRel) {
                DispatchQueue::main().exec_async(|| {
                    REPAINT_QUEUED.store(false, Ordering::Release);
                    let mtm = MainThreadMarker::new().expect("main queue");
                    if let Some(c) = controller(mtm) {
                        c.model_changed();
                    }
                });
            }
        });
        this
    }

    fn window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().window.borrow().clone()
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

    fn selected_repo_id(&self) -> Option<String> {
        let outline = self.ivars().outline.borrow().clone()?;
        let row = outline.selectedRow();
        if row >= 0 {
            if let Some(item) = outline.itemAtRow(row) {
                if let Some(i) = item.downcast_ref::<WTMItem>() {
                    return Some(i.kind().repo_id().to_string());
                }
            }
        }
        self.ivars()
            .shown
            .borrow()
            .as_ref()?
            .repos
            .first()
            .map(|r| r.repo.id.clone())
    }

    // MARK: Window construction

    fn build_window(&self, mtm: MainThreadMarker) {
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
        window.setTitle(&ns("Worktree Manager"));
        window.setMinSize(NSSize::new(640.0, 400.0));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.setFrameAutosaveName(&ns("WTMMainWindow"));
        unsafe { window.setReleasedWhenClosed(false) };

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
        unsafe {
            objc2_foundation::NSNotificationCenter::defaultCenter()
                .addObserver_selector_name_object(
                    self.as_ref(),
                    sel!(clipFrameChanged:),
                    Some(objc2_app_kit::NSViewFrameDidChangeNotification),
                    Some(&clip),
                );
        }
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);

        // Notice bar (hidden until there is something to say).
        let notice_bar = NSView::new(mtm);
        notice_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        notice_bar.setHidden(true);
        let notice_label = NSTextField::wrappingLabelWithString(&ns(""), mtm);
        notice_label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        notice_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        notice_label.setSelectable(true);
        // A notice must never resize the window: git output can run to
        // hundreds of lines. Three lines here; the rest behind "Details…".
        notice_label.setAlignment(objc2_app_kit::NSTextAlignment::Left);
        notice_label.setMaximumNumberOfLines(2);
        notice_label.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
        notice_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Vertical,
        );
        notice_bar
            .heightAnchor()
            .constraintLessThanOrEqualToConstant(64.0)
            .setActive(true);
        let details = unsafe {
            NSButton::buttonWithTitle_target_action(
                &ns("Details…"),
                Some(self.as_ref()),
                Some(sel!(showNoticeDetails:)),
                mtm,
            )
        };
        details.setControlSize(NSControlSize::Small);
        details.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        details.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        details.setTranslatesAutoresizingMaskIntoConstraints(false);
        details.setHidden(true);
        let close = unsafe {
            NSButton::buttonWithImage_target_action(
                &symbol("xmark", "Dismiss").unwrap(),
                Some(self.as_ref()),
                Some(sel!(dismissNotice:)),
                mtm,
            )
        };
        close.setBordered(false);
        close.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        close.setControlSize(NSControlSize::Small);
        close.setTranslatesAutoresizingMaskIntoConstraints(false);
        notice_bar.addSubview(&notice_label);
        notice_bar.addSubview(&details);
        notice_bar.addSubview(&close);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            notice_label
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&notice_bar.leadingAnchor(), 20.0),
            notice_label
                .topAnchor()
                .constraintEqualToAnchor_constant(&notice_bar.topAnchor(), 8.0),
            notice_label
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&notice_bar.bottomAnchor(), -8.0),
            details
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&notice_label.trailingAnchor(), 8.0),
            details
                .centerYAnchor()
                .constraintEqualToAnchor(&notice_bar.centerYAnchor()),
            close
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&details.trailingAnchor(), 4.0),
            close
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&notice_bar.trailingAnchor(), -12.0),
            close
                .centerYAnchor()
                .constraintEqualToAnchor(&notice_bar.centerYAnchor()),
        ]));

        // Empty state.
        let empty = NSStackView::new(mtm);
        empty.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        empty.setAlignment(NSLayoutAttribute::CenterX);
        empty.setSpacing(10.0);
        empty.setTranslatesAutoresizingMaskIntoConstraints(false);
        let title = NSTextField::labelWithString(&ns("No repositories yet"), mtm);
        title.setFont(Some(&NSFont::systemFontOfSize_weight(
            16.0,
            crate::util::SEMIBOLD,
        )));
        let sub = secondary_label("Add a git repository to see its worktrees here.", 12.0, mtm);
        let add = unsafe {
            NSButton::buttonWithTitle_target_action(
                &ns("Add Repository…"),
                Some(self.as_ref()),
                Some(sel!(addRepo:)),
                mtm,
            )
        };
        add.setKeyEquivalent(&ns("\r"));
        empty.addArrangedSubview(&title);
        empty.addArrangedSubview(&sub);
        empty.addArrangedSubview(&add);
        empty.setHidden(true);

        // Refresh spinner lives in the empty-state stack's sibling column.
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
        content.addSubview(&empty);
        content.addSubview(&spinner);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            empty
                .centerXAnchor()
                .constraintEqualToAnchor(&content.centerXAnchor()),
            empty
                .centerYAnchor()
                .constraintEqualToAnchor_constant(&content.centerYAnchor(), -20.0),
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
        *iv.empty.borrow_mut() = Some(Retained::into_super(empty));
        *iv.notice_bar.borrow_mut() = Some(notice_bar);
        *iv.notice_label.borrow_mut() = Some(notice_label);
        *iv.notice_details.borrow_mut() = Some(details);
        *iv.refresh_spinner.borrow_mut() = Some(spinner);

        crate::menu::install(&NSApplication::sharedApplication(mtm), self.as_ref(), mtm);
        window.center();
        window.makeKeyAndOrderFront(None);
        self.fit_column();
    }

    // MARK: Model → view

    /// Called on the main thread whenever the core publishes a new snapshot.
    pub fn model_changed(&self) {
        let model = self.ivars().app.model();
        self.rebuild(false);
        self.update_chrome(&model);
    }

    fn update_chrome(&self, model: &Model) {
        let iv = self.ivars();
        if let Some(e) = iv.empty.borrow().as_ref() {
            e.setHidden(!model.repos.is_empty());
        }
        if let Some(s) = iv.scroll.borrow().as_ref() {
            s.setHidden(model.repos.is_empty());
        }
        if let Some(sp) = iv.refresh_spinner.borrow().as_ref() {
            if model.refreshing || model.fetching {
                unsafe { sp.startAnimation(None) };
            } else {
                unsafe { sp.stopAnimation(None) };
            }
            sp.setToolTip(Some(&ns(if model.fetching {
                "Fetching remotes…"
            } else {
                "Refreshing…"
            })));
        }
        if let (Some(bar), Some(label)) = (
            iv.notice_bar.borrow().as_ref(),
            iv.notice_label.borrow().as_ref(),
        ) {
            match &model.notice {
                Some(n) => {
                    label.setStringValue(&ns(&notice_summary(&n.text)));
                    let color = match n.tone {
                        Tone::Error => NSColor::systemRedColor(),
                        Tone::Info => NSColor::labelColor(),
                    };
                    label.setTextColor(Some(&color));
                    label.setToolTip(Some(&ns(&n.text)));
                    let long = n.text.lines().count() > 3 || n.text.len() > 240;
                    if let Some(d) = iv.notice_details.borrow().as_ref() {
                        d.setHidden(!long);
                    }
                    bar.setHidden(false);
                    if iv.notice_id.get() != n.id {
                        iv.notice_id.set(n.id);
                        if n.tone == Tone::Info {
                            // Informational notices fade once the tree shows the outcome.
                            let app = iv.app.clone();
                            let id = n.id;
                            let _ = DispatchQueue::main().after(
                                dispatch2::DispatchTime::NOW.time(5_000_000_000),
                                move || {
                                    if app.model().notice.as_ref().map(|x| x.id) == Some(id) {
                                        app.dispatch(Action::ClearNotice);
                                    }
                                },
                            );
                        }
                    }
                }
                None => bar.setHidden(true),
            }
        }
    }

    /// Build the display tree from the current model and apply the smallest
    /// outline update that covers the difference from what is on screen.
    fn rebuild(&self, query_changed: bool) {
        let mtm = MainThreadMarker::from(self);
        let iv = self.ivars();
        let model = iv.app.model();
        let query = iv.query.borrow().trim().to_lowercase();
        let searching = !query.is_empty();

        let mut items = iv.items.borrow_mut();
        let mut item_for = |kind: ItemKind| -> Retained<WTMItem> {
            items
                .entry(kind.key())
                .or_insert_with(|| WTMItem::new(kind, mtm))
                .clone()
        };

        let mut tree = Tree::default();
        for node in &model.repos {
            let mut rows: Vec<(bool, String, Retained<WTMItem>)> = Vec::new();
            for w in &node.worktrees {
                if searching && !matches(&query, w, &model.home) {
                    continue;
                }
                let slug = w.path.rsplit('/').next().unwrap_or("").to_string();
                rows.push((
                    w.is_main,
                    slug,
                    item_for(ItemKind::Worktree {
                        repo_id: node.repo.id.clone(),
                        path: w.path.clone(),
                    }),
                ));
            }
            for p in model.pending_for(&node.repo.id) {
                rows.push((
                    false,
                    wtm_core::paths::slugify_branch(&p.branch),
                    item_for(ItemKind::Pending {
                        repo_id: node.repo.id.clone(),
                        id: p.id,
                    }),
                ));
            }
            if searching && rows.is_empty() && node.error.is_none() {
                continue;
            }
            rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let repo_item = item_for(ItemKind::Repo {
                repo_id: node.repo.id.clone(),
            });
            tree.children.insert(
                repo_item.kind().key(),
                rows.into_iter().map(|r| r.2).collect(),
            );
            tree.roots.push(repo_item);
        }
        drop(items);

        let Some(outline) = iv.outline.borrow().clone() else {
            *iv.tree.borrow_mut() = tree;
            *iv.shown.borrow_mut() = Some(model);
            return;
        };

        let previous = iv.shown.borrow().clone();
        let old_tree = std::mem::take(&mut *iv.tree.borrow_mut());
        let roots_changed = old_tree.roots.len() != tree.roots.len()
            || old_tree
                .roots
                .iter()
                .zip(&tree.roots)
                .any(|(a, b)| Retained::as_ptr(a) != Retained::as_ptr(b));

        *iv.tree.borrow_mut() = tree;
        *iv.shown.borrow_mut() = Some(model.clone());

        let full = previous.is_none() || query_changed || roots_changed;
        if full {
            outline.reloadData();
            // Clone what the loop needs: expanding calls back into the
            // delegate, which borrows these cells again.
            let roots = iv.tree.borrow().roots.clone();
            let collapsed = iv.collapsed.borrow().clone();
            for root in &roots {
                let id = root.kind().repo_id().to_string();
                if searching || !collapsed.contains(&id) {
                    unsafe { outline.expandItem(Some(root)) };
                }
            }
            self.sync_row_styles();
            return;
        }

        // Same repos in the same order: reload only what differs.
        let previous = previous.expect("checked above");
        let roots = iv.tree.borrow().roots.clone();
        for root in &roots {
            let key = root.kind().key();
            let repo_id = root.kind().repo_id().to_string();
            let new_children = iv
                .tree
                .borrow()
                .children
                .get(&key)
                .cloned()
                .unwrap_or_default();
            let new_children = &new_children;
            let old_children = old_tree.children.get(&key);
            let children_changed = old_children
                .map(|old| {
                    old.len() != new_children.len()
                        || old
                            .iter()
                            .zip(new_children)
                            .any(|(a, b)| Retained::as_ptr(a) != Retained::as_ptr(b))
                })
                .unwrap_or(true);
            let (old_node, new_node) = (previous.repo(&repo_id), model.repo(&repo_id));
            if children_changed {
                unsafe { outline.reloadItem_reloadChildren(Some(root), true) };
                let is_collapsed = iv.collapsed.borrow().contains(&repo_id);
                if !is_collapsed {
                    unsafe { outline.expandItem(Some(root)) };
                }
                continue;
            }
            let header_changed = match (old_node, new_node) {
                (Some(a), Some(b)) => {
                    a.repo != b.repo
                        || a.worktrees.len() != b.worktrees.len()
                        || a.error != b.error
                        || a.loaded != b.loaded
                }
                _ => true,
            };
            if header_changed {
                unsafe { outline.reloadItem(Some(root)) };
            }
            let branches_changed =
                matches!((old_node, new_node), (Some(a), Some(b)) if a.branches != b.branches);
            for child in new_children {
                let changed = match child.kind() {
                    ItemKind::Worktree { path, .. } => {
                        branches_changed
                            || old_node.and_then(|n| n.worktree(&path))
                                != new_node.and_then(|n| n.worktree(&path))
                            || previous.busy_for(&path) != model.busy_for(&path)
                    }
                    ItemKind::Pending { id, .. } => {
                        previous.pending.iter().find(|p| p.id == id)
                            != model.pending.iter().find(|p| p.id == id)
                    }
                    ItemKind::Repo { .. } => false,
                };
                if changed {
                    unsafe { outline.reloadItem(Some(child)) };
                }
            }
        }
        self.sync_row_styles();
    }
}

/// What the notice bar shows of a message: its first two lines, with a count
/// of what "Details…" holds. Git's file lists can run to hundreds of lines and
/// must never size the window.
fn notice_summary(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 2 {
        return text.to_string();
    }
    format!(
        "{}\n{} … ({} more lines)",
        lines[0].trim_end(),
        lines[1].trim(),
        lines.len() - 2
    )
}

/// Case-insensitive substring match on branch or path, like the web UI.
fn matches(query: &str, w: &wtm_core::WorktreeInfo, home: &str) -> bool {
    if w.branch
        .as_deref()
        .map(|b| b.to_lowercase().contains(query))
        .unwrap_or(false)
    {
        return true;
    }
    w.path.to_lowercase().contains(query)
        || wtm_core::paths::tildify(&w.path, home)
            .to_lowercase()
            .contains(query)
}

#[cfg(test)]
mod tests {
    use super::notice_summary;

    #[test]
    fn summary_keeps_short_notices_and_folds_long_ones() {
        assert_eq!(notice_summary("Added a"), "Added a");
        assert_eq!(notice_summary("a\nb"), "a\nb");
        assert_eq!(
            notice_summary("error: x:\n  f1\n  f2\n  f3"),
            "error: x:\nf1 … (2 more lines)"
        );
    }
}
