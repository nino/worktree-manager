//! Row views for the outline: a repo header, a worktree row, and a "Creating…"
//! placeholder. Each is an `NSTableCellView` subclass that owns its subviews,
//! is recycled by the outline view via its identifier, and is re-configured in
//! place from the model — so a status change costs a few property sets, never
//! a view rebuild.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSCellImagePosition, NSColor, NSControlSize, NSFont,
    NSImageSymbolConfiguration, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired,
    NSPasteboard, NSPasteboardTypeString, NSPopUpButton, NSProgressIndicator,
    NSProgressIndicatorStyle, NSStackView, NSStackViewDistribution, NSTableCellView, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};
use wtm_core::{Action, App, Busy, Model, PendingCreation, RepoConfig, RepoNode, WorktreeInfo};

use crate::badge::{badges_for, Badge};
use crate::dialogs;
use crate::util::{label, mono_label, ns, secondary_label, symbol};

use crate::outline::CONTENT_START;
use crate::rowview::{CARD_GAP, CARD_MARGIN, PLATE_GAP, PLATE_INSET};

/// Row heights include the card geometry drawn by `RowView`: the gap above a
/// card for headers, the plate gaps for children.
pub const REPO_ROW_HEIGHT: f64 = CARD_GAP + 50.0;
pub const WORKTREE_ROW_HEIGHT: f64 = 2.0 * PLATE_GAP + 50.0;
pub const PENDING_ROW_HEIGHT: f64 = 2.0 * PLATE_GAP + 36.0;

/// Content insets, `(leading, trailing, top, bottom)`, so cell content lands
/// inside the card (headers) or the plate (children). Cells start just after
/// the disclosure column, which `OutlineView` moves inside the card, so the
/// leading inset is what remains to reach the plate's own padding.
const HEADER_INSETS: (f64, f64, f64, f64) =
    (CONTENT_START, CARD_MARGIN + 14.0, CARD_GAP + 7.0, 7.0);
const PLATE_INSETS: (f64, f64, f64, f64) = (
    CONTENT_START,
    CARD_MARGIN + PLATE_INSET + 12.0,
    PLATE_GAP + 5.0,
    PLATE_GAP + 5.0,
);

// MARK: Shared building blocks

fn hstack(spacing: f64, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    s.setAlignment(NSLayoutAttribute::CenterY);
    s.setSpacing(spacing);
    s.setDistribution(NSStackViewDistribution::Fill);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

fn vstack(spacing: f64, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    s.setAlignment(NSLayoutAttribute::Leading);
    s.setSpacing(spacing);
    s.setDistribution(NSStackViewDistribution::Fill);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

/// An empty view that soaks up leftover width, pushing what follows it to the
/// trailing edge.
fn spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    let v = NSView::new(mtm);
    v.setTranslatesAutoresizingMaskIntoConstraints(false);
    v.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    v.setContentCompressionResistancePriority_forOrientation(
        1.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    v
}

/// Pin `content` to all four edges of `cell` with `(leading, trailing, top,
/// bottom)` insets.
fn pin(cell: &NSView, content: &NSView, insets: (f64, f64, f64, f64)) {
    cell.addSubview(content);
    let (l, t, top, bottom) = insets;
    let c = [
        content
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), l),
        content
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -t),
        content
            .topAnchor()
            .constraintEqualToAnchor_constant(&cell.topAnchor(), top),
        // Bottom is a floor, not a pin: a card's last row is taller than its
        // plate (see `LAST_ROW_EXTRA`), and content must stay put in the plate.
        content
            .bottomAnchor()
            .constraintLessThanOrEqualToAnchor_constant(&cell.bottomAnchor(), -bottom),
    ];
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&c));
}

/// A borderless SF Symbol button.
fn icon_button(sym: &str, tooltip: &str, mtm: MainThreadMarker) -> Retained<NSButton> {
    let image = symbol(sym, tooltip).unwrap_or_default();
    let b = unsafe { NSButton::buttonWithImage_target_action(&image, None, None, mtm) };
    b.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    b.setBordered(false);
    b.setImagePosition(NSCellImagePosition::ImageOnly);
    b.setControlSize(NSControlSize::Small);
    b.setToolTip(Some(&ns(tooltip)));
    b.setSymbolConfiguration(Some(
        &NSImageSymbolConfiguration::configurationWithPointSize_weight(12.0, crate::util::MEDIUM),
    ));
    b.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    b.setContentHuggingPriority_forOrientation(
        NSLayoutPriorityRequired,
        NSLayoutConstraintOrientation::Horizontal,
    );
    b.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityRequired,
        NSLayoutConstraintOrientation::Horizontal,
    );
    b
}

/// Point a control at `target` / `action`.
fn wire(control: &NSButton, target: &AnyObject, action: objc2::runtime::Sel) {
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(action));
    }
}

fn copy_to_pasteboard(text: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    unsafe { pb.setString_forType(&ns(text), NSPasteboardTypeString) };
}

fn small_spinner(mtm: MainThreadMarker) -> Retained<NSProgressIndicator> {
    let p = NSProgressIndicator::new(mtm);
    p.setStyle(NSProgressIndicatorStyle::Spinning);
    p.setControlSize(NSControlSize::Small);
    p.setIndeterminate(true);
    p.setDisplayedWhenStopped(false);
    p.setTranslatesAutoresizingMaskIntoConstraints(false);
    p
}

// MARK: Repo header row

pub struct RepoCellIvars {
    app: App,
    repo_id: RefCell<String>,
    path: RefCell<String>,
    name: Retained<NSTextField>,
    meta: Retained<NSTextField>,
    path_label: Retained<NSTextField>,
    error: Retained<NSTextField>,
    spinner: Retained<NSProgressIndicator>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMRepoCell"]
    #[ivars = RepoCellIvars]
    pub struct RepoCell;

    impl RepoCell {
        #[unsafe(method(newWorktree:))]
        fn new_worktree(&self, _sender: Option<&AnyObject>) {
            if let Some(window) = self.window() {
                dialogs::create_worktree(&self.ivars().app, &window, &self.ivars().repo_id.borrow());
            }
        }

        #[unsafe(method(repoSettings:))]
        fn repo_settings(&self, _sender: Option<&AnyObject>) {
            if let Some(window) = self.window() {
                dialogs::repo_settings(&self.ivars().app, &window, &self.ivars().repo_id.borrow());
            }
        }

        #[unsafe(method(copyPath:))]
        fn copy_path(&self, _sender: Option<&AnyObject>) {
            copy_to_pasteboard(&self.ivars().path.borrow());
        }
    }
);

impl RepoCell {
    pub const IDENTIFIER: &'static str = "wtm.repo";

    pub fn new(app: App, mtm: MainThreadMarker) -> Retained<Self> {
        let name = label("", mtm);
        name.setFont(Some(&NSFont::systemFontOfSize_weight(
            14.0,
            crate::util::SEMIBOLD,
        )));
        let meta = secondary_label("", 11.0, mtm);
        let path_label = mono_label("", 10.5, mtm);
        path_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let error = secondary_label("", 11.0, mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        error.setHidden(true);
        let spinner = small_spinner(mtm);

        let this = mtm.alloc::<Self>().set_ivars(RepoCellIvars {
            app,
            repo_id: RefCell::new(String::new()),
            path: RefCell::new(String::new()),
            name: name.clone(),
            meta: meta.clone(),
            path_label: path_label.clone(),
            error: error.clone(),
            spinner: spinner.clone(),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(400.0, REPO_ROW_HEIGHT))]
        };
        this.setIdentifier(Some(&ns(Self::IDENTIFIER)));
        let target: &AnyObject = this.as_ref();

        let copy = icon_button("doc.on.doc", "Copy path", mtm);
        wire(&copy, target, sel!(copyPath:));
        let new_wt = unsafe {
            NSButton::buttonWithTitle_target_action(
                &ns("New Worktree"),
                Some(target),
                Some(sel!(newWorktree:)),
                mtm,
            )
        };
        new_wt.setControlSize(NSControlSize::Small);
        new_wt.setBezelStyle(NSBezelStyle::Push);
        new_wt.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        let settings = icon_button("gearshape", "Repo settings", mtm);
        wire(&settings, target, sel!(repoSettings:));

        let line1 = hstack(8.0, mtm);
        line1.addArrangedSubview(&name);
        line1.addArrangedSubview(&meta);
        line1.addArrangedSubview(&spinner);
        let line2 = hstack(4.0, mtm);
        line2.addArrangedSubview(&path_label);
        line2.addArrangedSubview(&copy);
        line2.addArrangedSubview(&error);
        let text = vstack(1.0, mtm);
        text.addArrangedSubview(&line1);
        text.addArrangedSubview(&line2);
        text.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        // Buttons are centred on the whole header, not on its first line.
        let row = hstack(8.0, mtm);
        row.addArrangedSubview(&text);
        row.addArrangedSubview(&spacer(mtm));
        row.addArrangedSubview(&new_wt);
        row.addArrangedSubview(&settings);
        pin(&this, &row, HEADER_INSETS);
        this
    }

    pub fn configure(&self, node: &RepoNode, model: &Model, visible: usize, searching: bool) {
        let iv = self.ivars();
        *iv.repo_id.borrow_mut() = node.repo.id.clone();
        *iv.path.borrow_mut() = node.repo.path.clone();
        iv.name.setStringValue(&ns(&node.repo.name));
        let total = node.worktrees.len();
        let plural = if total == 1 { "" } else { "s" };
        let count = if searching {
            format!("{visible} of {total} worktree{plural}")
        } else {
            format!("{total} worktree{plural}")
        };
        iv.meta
            .setStringValue(&ns(&format!("{} · {count}", node.repo.main_branch)));
        let shown = wtm_core::paths::tildify(&node.repo.path, &model.home);
        iv.path_label.setStringValue(&ns(&shown));
        iv.path_label.setToolTip(Some(&ns(&node.repo.path)));
        match &node.error {
            Some(e) => {
                iv.error.setStringValue(&ns(e));
                iv.error.setHidden(false);
            }
            None => iv.error.setHidden(true),
        }
        if !node.loaded {
            unsafe { iv.spinner.startAnimation(None) };
        } else {
            unsafe { iv.spinner.stopAnimation(None) };
        }
    }
}

// MARK: Worktree row

pub struct WorktreeCellIvars {
    app: App,
    repo_id: RefCell<String>,
    path: RefCell<String>,
    branch: RefCell<Option<String>>,
    branches: RefCell<Vec<String>>,
    popup: Retained<NSPopUpButton>,
    detached: Retained<NSTextField>,
    copy_branch: Retained<NSButton>,
    badges: Retained<NSStackView>,
    badge_views: RefCell<Vec<Retained<Badge>>>,
    spinner: Retained<NSProgressIndicator>,
    busy: Retained<NSTextField>,
    path_label: Retained<NSTextField>,
    push: Retained<NSButton>,
    pull: Retained<NSButton>,
    merge: Retained<NSButton>,
    editor: Retained<NSButton>,
    terminal: Retained<NSButton>,
    reveal: Retained<NSButton>,
    delete: Retained<NSButton>,
    /// Set while the popup is being filled so a programmatic selection never
    /// looks like a user's switch request.
    filling: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMWorktreeCell"]
    #[ivars = WorktreeCellIvars]
    pub struct WorktreeCell;

    impl WorktreeCell {
        #[unsafe(method(push:))]
        fn push(&self, _s: Option<&AnyObject>) {
            let (repo_id, path) = self.ids();
            self.ivars().app.dispatch(Action::Push { repo_id, path });
        }

        #[unsafe(method(pull:))]
        fn pull(&self, _s: Option<&AnyObject>) {
            let (repo_id, path) = self.ids();
            self.ivars().app.dispatch(Action::Pull { repo_id, path });
        }

        #[unsafe(method(pullMain:))]
        fn pull_main(&self, _s: Option<&AnyObject>) {
            let (repo_id, path) = self.ids();
            self.ivars().app.dispatch(Action::PullMain { repo_id, path });
        }

        #[unsafe(method(openEditor:))]
        fn open_editor(&self, _s: Option<&AnyObject>) {
            self.ivars().app.dispatch(Action::OpenInEditor(self.ids().1));
        }

        #[unsafe(method(openTerminal:))]
        fn open_terminal(&self, _s: Option<&AnyObject>) {
            self.ivars().app.dispatch(Action::OpenInTerminal(self.ids().1));
        }

        #[unsafe(method(reveal:))]
        fn reveal(&self, _s: Option<&AnyObject>) {
            self.ivars().app.dispatch(Action::Reveal(self.ids().1));
        }

        #[unsafe(method(copyPath:))]
        fn copy_path(&self, _s: Option<&AnyObject>) {
            copy_to_pasteboard(&self.ids().1);
        }

        #[unsafe(method(copyBranch:))]
        fn copy_branch(&self, _s: Option<&AnyObject>) {
            if let Some(b) = self.ivars().branch.borrow().as_deref() {
                copy_to_pasteboard(b);
            }
        }

        #[unsafe(method(deleteWorktree:))]
        fn delete_worktree(&self, _s: Option<&AnyObject>) {
            let (repo_id, path) = self.ids();
            if let Some(window) = self.window() {
                dialogs::confirm_delete(&self.ivars().app, &window, &repo_id, &path);
            }
        }

        #[unsafe(method(switchBranch:))]
        fn switch_branch(&self, _s: Option<&AnyObject>) {
            let iv = self.ivars();
            if iv.filling.get() {
                return;
            }
            let Some(title) = iv.popup.titleOfSelectedItem() else { return };
            let chosen = title.to_string();
            if iv.branch.borrow().as_deref() == Some(chosen.as_str()) {
                return;
            }
            let (repo_id, path) = self.ids();
            iv.app.dispatch(Action::Switch { repo_id, path, branch: chosen });
        }
    }
);

impl WorktreeCell {
    pub const IDENTIFIER: &'static str = "wtm.worktree";

    fn ids(&self) -> (String, String) {
        (
            self.ivars().repo_id.borrow().clone(),
            self.ivars().path.borrow().clone(),
        )
    }

    pub fn new(app: App, mtm: MainThreadMarker) -> Retained<Self> {
        let popup = NSPopUpButton::initWithFrame_pullsDown(
            mtm.alloc(),
            NSRect::new(NSPoint::ZERO, NSSize::new(160.0, 20.0)),
            false,
        );
        popup.setBordered(false);
        popup.setControlSize(NSControlSize::Small);
        popup.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
            12.0,
            crate::util::SEMIBOLD,
        )));
        popup.setTranslatesAutoresizingMaskIntoConstraints(false);
        popup.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow + 10.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        popup.setToolTip(Some(&ns("Switch branch")));
        let detached = label("(detached)", mtm);
        detached.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
            12.0,
            crate::util::SEMIBOLD,
        )));
        detached.setHidden(true);
        let badges = hstack(4.0, mtm);
        badges.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let spinner = small_spinner(mtm);
        let busy = secondary_label("", 11.0, mtm);
        busy.setHidden(true);
        let path_label = mono_label("", 10.5, mtm);
        path_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let copy_branch = icon_button("doc.on.doc", "Copy branch name", mtm);
        let copy_path = icon_button("doc.on.doc", "Copy path", mtm);
        let push = icon_button("arrow.up.to.line", "Push", mtm);
        let pull = icon_button("arrow.down.to.line", "Pull (fast-forward only)", mtm);
        let merge = icon_button(
            "arrow.triangle.merge",
            "Pull the primary branch into this branch",
            mtm,
        );
        let editor = icon_button(
            "chevron.left.forwardslash.chevron.right",
            "Open in editor",
            mtm,
        );
        let terminal = icon_button("terminal", "Open in terminal", mtm);
        let reveal = icon_button("folder", "Reveal in Finder", mtm);
        let delete = icon_button("trash", "Delete worktree", mtm);
        delete.setContentTintColor(Some(&NSColor::systemRedColor()));

        let this = mtm.alloc::<Self>().set_ivars(WorktreeCellIvars {
            app,
            repo_id: RefCell::new(String::new()),
            path: RefCell::new(String::new()),
            branch: RefCell::new(None),
            branches: RefCell::new(Vec::new()),
            popup: popup.clone(),
            detached: detached.clone(),
            copy_branch: copy_branch.clone(),
            badges: badges.clone(),
            badge_views: RefCell::new(Vec::new()),
            spinner: spinner.clone(),
            busy: busy.clone(),
            path_label: path_label.clone(),
            push: push.clone(),
            pull: pull.clone(),
            merge: merge.clone(),
            editor: editor.clone(),
            terminal: terminal.clone(),
            reveal: reveal.clone(),
            delete: delete.clone(),
            filling: Cell::new(false),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(400.0, WORKTREE_ROW_HEIGHT))]
        };
        this.setIdentifier(Some(&ns(Self::IDENTIFIER)));
        let target: &AnyObject = this.as_ref();
        unsafe {
            popup.setTarget(Some(target));
            popup.setAction(Some(sel!(switchBranch:)));
        }
        for (b, action) in [
            (&copy_branch, sel!(copyBranch:)),
            (&copy_path, sel!(copyPath:)),
            (&push, sel!(push:)),
            (&pull, sel!(pull:)),
            (&merge, sel!(pullMain:)),
            (&editor, sel!(openEditor:)),
            (&terminal, sel!(openTerminal:)),
            (&reveal, sel!(reveal:)),
            (&delete, sel!(deleteWorktree:)),
        ] {
            wire(b, target, action);
        }

        let line1 = hstack(6.0, mtm);
        line1.addArrangedSubview(&popup);
        line1.addArrangedSubview(&detached);
        line1.addArrangedSubview(&copy_branch);
        line1.addArrangedSubview(&spacer(mtm));
        line1.addArrangedSubview(&spinner);
        line1.addArrangedSubview(&busy);
        line1.addArrangedSubview(&badges);
        let line2 = hstack(2.0, mtm);
        line2.addArrangedSubview(&path_label);
        line2.addArrangedSubview(&copy_path);
        line2.addArrangedSubview(&spacer(mtm));
        for (i, b) in [&push, &pull, &merge, &editor, &terminal, &reveal, &delete]
            .into_iter()
            .enumerate()
        {
            line2.addArrangedSubview(b);
            if i == 2 || i == 5 {
                line2.setCustomSpacing_afterView(10.0, b);
            }
        }
        let col = vstack(3.0, mtm);
        col.addArrangedSubview(&line1);
        col.addArrangedSubview(&line2);
        line1
            .widthAnchor()
            .constraintEqualToAnchor(&col.widthAnchor())
            .setActive(true);
        line2
            .widthAnchor()
            .constraintEqualToAnchor(&col.widthAnchor())
            .setActive(true);
        pin(&this, &col, PLATE_INSETS);
        this
    }

    pub fn configure(
        &self,
        repo: &RepoConfig,
        w: &WorktreeInfo,
        branches: &[String],
        busy: Option<Busy>,
        home: &str,
    ) {
        let iv = self.ivars();
        *iv.repo_id.borrow_mut() = repo.id.clone();
        *iv.path.borrow_mut() = w.path.clone();
        *iv.branch.borrow_mut() = w.branch.clone();

        // Branch picker.
        match &w.branch {
            Some(b) => {
                iv.popup.setHidden(false);
                iv.detached.setHidden(true);
                iv.filling.set(true);
                if *iv.branches.borrow() != branches
                    || !iv.popup.itemTitles().iter().any(|t| t.to_string() == *b)
                {
                    iv.popup.removeAllItems();
                    let mut titles: Vec<Retained<NSString>> =
                        branches.iter().map(|s| ns(s)).collect();
                    if !branches.iter().any(|x| x == b) {
                        titles.insert(0, ns(b));
                    }
                    iv.popup
                        .addItemsWithTitles(&NSArray::from_retained_slice(&titles));
                    *iv.branches.borrow_mut() = branches.to_vec();
                }
                iv.popup.selectItemWithTitle(&ns(b));
                iv.filling.set(false);
            }
            None => {
                iv.popup.setHidden(true);
                iv.detached.setHidden(false);
            }
        }
        iv.copy_branch.setHidden(w.branch.is_none());

        // Badges: reuse existing views, add or drop the difference.
        let wanted = badges_for(w, &repo.main_branch);
        let mut views = iv.badge_views.borrow_mut();
        let mtm = MainThreadMarker::from(self);
        while views.len() > wanted.len() {
            let v = views.pop().unwrap();
            iv.badges.removeArrangedSubview(&v);
            v.removeFromSuperview();
        }
        for (i, (text, tone, tip)) in wanted.iter().enumerate() {
            if i < views.len() {
                views[i].set(text, *tone, tip);
            } else {
                let b = Badge::new(text, *tone, tip, mtm);
                iv.badges.addArrangedSubview(&b);
                views.push(b);
            }
        }

        // Path.
        iv.path_label
            .setStringValue(&ns(&wtm_core::paths::tildify(&w.path, home)));
        iv.path_label.setToolTip(Some(&ns(&w.path)));

        // Busy state.
        match busy {
            Some(b) => {
                unsafe { iv.spinner.startAnimation(None) };
                iv.busy.setStringValue(&ns(b.label()));
                iv.busy.setHidden(false);
            }
            None => {
                unsafe { iv.spinner.stopAnimation(None) };
                iv.busy.setHidden(true);
            }
        }
        let missing = w.prunable;
        let is_busy = busy.is_some();
        iv.popup.setEnabled(!is_busy && !missing);
        for b in [&iv.push, &iv.pull, &iv.merge] {
            b.setEnabled(!is_busy && !missing);
        }
        for b in [&iv.editor, &iv.terminal, &iv.reveal] {
            b.setEnabled(!missing);
        }
        iv.delete.setEnabled(!is_busy);
        iv.delete.setHidden(w.is_main);
        iv.merge.setToolTip(Some(&ns(&format!(
            "Pull {} into this branch",
            repo.main_branch
        ))));
    }
}

// MARK: Pending creation row

pub struct PendingCellIvars {
    app: App,
    id: Cell<u64>,
    branch: Retained<NSTextField>,
    spinner: Retained<NSProgressIndicator>,
    status: Retained<NSTextField>,
    dismiss: Retained<NSButton>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMPendingCell"]
    #[ivars = PendingCellIvars]
    pub struct PendingCell;

    impl PendingCell {
        #[unsafe(method(dismiss:))]
        fn dismiss(&self, _s: Option<&AnyObject>) {
            self.ivars().app.dispatch(Action::DismissCreation(self.ivars().id.get()));
        }
    }
);

impl PendingCell {
    pub const IDENTIFIER: &'static str = "wtm.pending";

    pub fn new(app: App, mtm: MainThreadMarker) -> Retained<Self> {
        let branch = label("", mtm);
        branch.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
            12.0,
            crate::util::SEMIBOLD,
        )));
        let spinner = small_spinner(mtm);
        let status = secondary_label("Creating…", 11.0, mtm);
        let dismiss = icon_button("xmark", "Dismiss", mtm);
        let this = mtm.alloc::<Self>().set_ivars(PendingCellIvars {
            app,
            id: Cell::new(0),
            branch: branch.clone(),
            spinner: spinner.clone(),
            status: status.clone(),
            dismiss: dismiss.clone(),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(400.0, PENDING_ROW_HEIGHT))]
        };
        this.setIdentifier(Some(&ns(Self::IDENTIFIER)));
        let target: &AnyObject = this.as_ref();
        wire(&dismiss, target, sel!(dismiss:));
        let line = hstack(8.0, mtm);
        line.addArrangedSubview(&branch);
        line.addArrangedSubview(&spinner);
        line.addArrangedSubview(&status);
        line.addArrangedSubview(&spacer(mtm));
        line.addArrangedSubview(&dismiss);
        pin(&this, &line, PLATE_INSETS);
        this
    }

    pub fn configure(&self, p: &PendingCreation) {
        let iv = self.ivars();
        iv.id.set(p.id);
        iv.branch.setStringValue(&ns(&p.branch));
        match &p.error {
            Some(e) => {
                unsafe { iv.spinner.stopAnimation(None) };
                iv.status.setStringValue(&ns(e));
                iv.status.setTextColor(Some(&NSColor::systemRedColor()));
                iv.dismiss.setHidden(false);
            }
            None => {
                unsafe { iv.spinner.startAnimation(None) };
                iv.status.setStringValue(&ns("Creating…"));
                iv.status
                    .setTextColor(Some(&NSColor::secondaryLabelColor()));
                iv.dismiss.setHidden(true);
            }
        }
    }
}
