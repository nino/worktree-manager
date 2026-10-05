//! Row views for the outline: a repo header, a worktree row, and a "Creating…"
//! placeholder. Each is an `NSTableCellView` subclass that owns its subviews,
//! is recycled by the outline view via its identifier, and is re-configured in
//! place from the view's typed rows — so a status change costs a few property
//! sets, never a view rebuild.
//!
//! A cell keeps only its row's key. Its buttons look the row up in the list
//! the controller last rendered when they are pressed, so they always run
//! that render's handlers, never the ones the cell was configured with.

use std::cell::RefCell;
use std::rc::Rc;

use block2::RcBlock;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSBezelStyle, NSButton, NSCellImagePosition,
    NSColor, NSControlSize, NSDraggingImageComponent, NSDraggingImageComponentIconKey, NSFont,
    NSImage, NSLayoutAttribute, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSProgressIndicator,
    NSProgressIndicatorStyle, NSStackView, NSStackViewDistribution, NSTableCellView, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};
use wtm_toolkit::{Icon, PendingRow, RepoHeader, Rich, Tint, WorktreeRow};

use crate::badge::{Badge, BadgeTone};
use crate::branchlabel::rich_label;
use crate::button::{Button, IconButton, PillButton};
use crate::controller;
use crate::elements::{icon_button, Callback};
use crate::util::{label, mono_label, ns, render, secondary_label, symbol_raised};

use crate::outline::CONTENT_START;
use crate::rowview::{RowStyle, RowView, CARD_GAP, CARD_MARGIN, PLATE_GAP, PLATE_INSET};

/// Row heights include the card geometry drawn by `RowView`: the gap above a
/// card for headers, the plate gaps for children. A card's first and last
/// child rows are taller still (see `set_lead` and `LAST_ROW_EXTRA`).
pub const REPO_ROW_HEIGHT: f64 = CARD_GAP + 50.0;
pub const WORKTREE_ROW_HEIGHT: f64 = 2.0 * PLATE_GAP + 50.0;
pub const PENDING_ROW_HEIGHT: f64 = 2.0 * PLATE_GAP + 36.0;

/// Content insets, `(leading, trailing, top, bottom)`, so cell content lands
/// inside the card (headers) or the plate (children). Cells start just after
/// the disclosure column, which `OutlineView` moves inside the card, so the
/// leading inset is what remains to reach the plate's own padding.
const HEADER_INSETS: (f64, f64, f64, f64) =
    (CONTENT_START, CARD_MARGIN + 14.0, CARD_GAP + 9.0, 7.0);
/// How far the branch button's chevrons are lifted so they centre on the
/// branch name rather than on its line box, and how far they sit from it
/// (see `symbol_raised`).
const CHEVRON_LIFT: f64 = 1.25;
const CHEVRON_GAP: f64 = 3.0;

const PLATE_INSETS: (f64, f64, f64, f64) = (
    CARD_MARGIN + PLATE_INSET + 12.0,
    CARD_MARGIN + PLATE_INSET + 12.0,
    PLATE_GAP + 8.0,
    PLATE_GAP + 8.0,
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
/// bottom)` insets. Returns the top constraint, whose constant moves the
/// content down in a card's first row (see `set_lead`).
fn pin(
    cell: &NSView,
    content: &NSView,
    insets: (f64, f64, f64, f64),
) -> Retained<NSLayoutConstraint> {
    cell.addSubview(content);
    let (l, t, top, bottom) = insets;
    let top_c = content
        .topAnchor()
        .constraintEqualToAnchor_constant(&cell.topAnchor(), top);
    let c = [
        content
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), l),
        content
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -t),
        top_c.clone(),
        // Bottom is a floor, not a pin: a card's last row is taller than its
        // plate (see `LAST_ROW_EXTRA`), and content must stay put in the plate.
        content
            .bottomAnchor()
            .constraintLessThanOrEqualToAnchor_constant(&cell.bottomAnchor(), -bottom),
    ];
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&c));
    top_c
}

/// Cells on a plate: their content follows the plate down by the well's
/// lead-in when they are a card's first row.
pub trait PlateCell {
    fn set_lead(&self, lead: f64);
}

/// A cell's row key, shared with its buttons' callbacks: a recycled cell
/// shows another row, and its buttons must act on that one.
type RowKey = Rc<RefCell<String>>;

/// Point `button` at a callback that runs `f` with the cell's current key.
/// The callback is returned for the cell to keep: a button holds its target
/// weakly.
fn on_press(
    button: &NSButton,
    key: &RowKey,
    f: impl Fn(&str) + 'static,
    mtm: MainThreadMarker,
) -> Retained<Callback> {
    let target = Callback::new(mtm);
    let key = key.clone();
    target.set(move || {
        let key = key.borrow().clone();
        f(&key)
    });
    target.attach(button);
    target
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
    key: RowKey,
    name: Retained<NSTextField>,
    meta: Retained<NSTextField>,
    path_label: Retained<NSTextField>,
    error: Retained<NSTextField>,
    spinner: Retained<NSProgressIndicator>,
    new_worktree: Retained<Button>,
    /// Its buttons' targets; a button holds its target weakly.
    _targets: Vec<Retained<Callback>>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMRepoCell"]
    #[ivars = RepoCellIvars]
    pub struct RepoCell;

    impl RepoCell {
        /// `OutlineView` moves the disclosure chevron inside the card, which
        /// puts it under this cell: the cell spans the whole row and AppKit
        /// adds it above the chevron's button, so every click on the chevron
        /// landed here and only selected the row. The leading strip the
        /// content keeps clear for the chevron is left to what is beneath.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test_past_chevron(point)
        }

        /// What a repo dragged to a new place in the list looks like.
        #[unsafe(method_id(draggingImageComponents))]
        fn dragging_image_components(&self) -> Retained<NSArray<NSDraggingImageComponent>> {
            self.drag_image()
        }
    }
);

impl RepoCell {
    pub const IDENTIFIER: &'static str = "wtm.repo";

    fn hit_test_past_chevron(&self, point: NSPoint) -> Option<Retained<NSView>> {
        // `point` is in the superview's coordinates, as the frame is.
        if point.x - self.frame().origin.x < CONTENT_START {
            return None;
        }
        unsafe { msg_send![super(self), hitTest: point] }
    }

    /// The header as it looks on screen, drawn as a closed card: the whole
    /// card is what moves, and an open one's header ends in a flat edge.
    /// `NSTableCellView` builds its image from the `textField` and
    /// `imageView` outlets, which this cell does not set, and the card is
    /// drawn by the row view beneath the cell, so the default image would be
    /// empty.
    fn drag_image(&self) -> Retained<NSArray<NSDraggingImageComponent>> {
        let row = unsafe { self.superview() };
        let rep = row.as_deref().and_then(|row| {
            let Some(card) = row.downcast_ref::<RowView>() else {
                return render(row);
            };
            let style = card.style();
            card.set_style(RowStyle::Header { closed: true });
            let rep = render(row);
            card.set_style(style);
            rep
        });
        let (Some(row), Some(rep)) = (row, rep) else {
            return unsafe { msg_send![super(self), draggingImageComponents] };
        };
        let mtm = MainThreadMarker::from(self);
        let bounds = row.bounds();
        let image = NSImage::initWithSize(mtm.alloc(), bounds.size);
        image.addRepresentation(&rep);
        let component = NSDraggingImageComponent::initWithKey(mtm.alloc(), unsafe {
            NSDraggingImageComponentIconKey
        });
        unsafe { component.setContents(Some(&image)) };
        component.setFrame(self.convertRect_fromView(bounds, Some(&row)));
        NSArray::from_retained_slice(&[component])
    }

    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
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
        let new_wt = Button::with_title(&ns("New Worktree"), None, sel!(performClick:), mtm);
        new_wt.setControlSize(NSControlSize::Small);
        new_wt.setBezelStyle(NSBezelStyle::Push);
        new_wt.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        let copy = icon_button(Icon::Copy, "Copy path", mtm);
        let settings = icon_button(Icon::Settings, "Repo settings", mtm);

        let key: RowKey = Rc::default();
        let targets = vec![
            on_press(
                &copy,
                &key,
                |k| controller::with_header(k, |h| h.on_copy_path.call(())),
                mtm,
            ),
            on_press(
                &new_wt,
                &key,
                |k| controller::with_header(k, |h| h.on_new_worktree.call(())),
                mtm,
            ),
            on_press(
                &settings,
                &key,
                |k| controller::with_header(k, |h| h.on_settings.call(())),
                mtm,
            ),
        ];
        let this = mtm.alloc::<Self>().set_ivars(RepoCellIvars {
            key,
            name: name.clone(),
            meta: meta.clone(),
            path_label: path_label.clone(),
            error: error.clone(),
            spinner: spinner.clone(),
            new_worktree: new_wt.clone(),
            _targets: targets,
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(400.0, REPO_ROW_HEIGHT))]
        };
        this.setIdentifier(Some(&ns(Self::IDENTIFIER)));

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
        let _ = pin(&this, &row, HEADER_INSETS);
        this
    }

    pub fn configure(&self, key: &str, h: &RepoHeader) {
        let iv = self.ivars();
        *iv.key.borrow_mut() = key.to_string();
        iv.name.setStringValue(&ns(&h.name));
        iv.meta.setStringValue(&ns(&h.meta));
        iv.path_label.setStringValue(&ns(&h.path));
        iv.path_label.setToolTip(Some(&ns(&h.path_full)));
        match &h.error {
            Some(e) => {
                iv.error.setStringValue(&ns(e));
                iv.error.setHidden(false);
            }
            None => iv.error.setHidden(true),
        }
        iv.new_worktree.setEnabled(h.can_create);
        if h.loading {
            unsafe { iv.spinner.startAnimation(None) };
        } else {
            unsafe { iv.spinner.stopAnimation(None) };
        }
    }
}

// MARK: Worktree row

pub struct WorktreeCellIvars {
    key: RowKey,
    branch: RefCell<Rich>,
    /// The branch button: its title is the branch (or "(detached)"), a click
    /// opens the fuzzy picker.
    picker: Retained<PillButton>,
    copy_branch: Retained<IconButton>,
    badges: Retained<NSStackView>,
    badge_views: RefCell<Vec<Retained<Badge>>>,
    spinner: Retained<NSProgressIndicator>,
    busy: Retained<NSTextField>,
    path_label: Retained<NSTextField>,
    /// The row's action buttons, by the id of the action each shows.
    actions: Vec<(&'static str, Retained<IconButton>)>,
    /// The delete button is tinted by appearance (see `paint_delete_tint`).
    danger: RefCell<bool>,
    top: RefCell<Option<Retained<NSLayoutConstraint>>>,
    /// Its buttons' targets; a button holds its target weakly.
    _targets: Vec<Retained<Callback>>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMWorktreeCell"]
    #[ivars = WorktreeCellIvars]
    pub struct WorktreeCell;

    impl WorktreeCell {
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn view_did_change_effective_appearance(&self) {
            self.paint_branch_title();
            self.paint_delete_tint();
        }
    }
);

/// The actions a worktree row lays out, in order, with the wider gap after
/// the third and the sixth. `wtm_ui::list` names them.
const ACTIONS: [(&str, Icon); 7] = [
    ("push", Icon::Push),
    ("pull", Icon::Pull),
    ("merge", Icon::Merge),
    ("editor", Icon::Editor),
    ("terminal", Icon::Terminal),
    ("reveal", Icon::Folder),
    ("delete", Icon::Delete),
];

impl WorktreeCell {
    /// `primary_ink` snapshots when dark, so the title is rebuilt under this
    /// view's appearance.
    fn paint_branch_title(&self) {
        let iv = self.ivars();
        let enabled = iv.picker.isEnabled();
        let picker = iv.picker.clone();
        let shown = iv.branch.borrow().clone();
        self.effectiveAppearance()
            .performAsCurrentDrawingAppearance(&RcBlock::new(move || {
                let ink = if enabled {
                    crate::util::primary_ink()
                } else {
                    NSColor::disabledControlTextColor()
                };
                picker.setAttributedTitle(&rich_label(
                    &shown,
                    &crate::util::branch_font(),
                    &ink,
                    None,
                ));
            }));
    }

    /// Red in light. Dark gets the other icons' grey: there a red trash per
    /// row is a column of the loudest colour in the palette, and delete
    /// already confirms. Chosen under this view's appearance, so it is redone
    /// when that changes.
    fn paint_delete_tint(&self) {
        let iv = self.ivars();
        let danger = *iv.danger.borrow();
        let Some(delete) = self.action_button("delete") else {
            return;
        };
        self.effectiveAppearance()
            .performAsCurrentDrawingAppearance(&RcBlock::new(move || {
                let tint = if danger && !crate::util::drawing_dark() {
                    NSColor::systemRedColor()
                } else {
                    NSColor::secondaryLabelColor()
                };
                delete.setContentTintColor(Some(&tint));
            }));
    }

    fn action_button(&self, id: &str) -> Option<Retained<IconButton>> {
        self.ivars()
            .actions
            .iter()
            .find(|(a, _)| *a == id)
            .map(|(_, b)| b.clone())
    }

    /// The branch button, which the picker hangs from.
    pub fn branch_button(&self) -> Retained<NSView> {
        Retained::into_super(Retained::into_super(Retained::into_super(
            Retained::into_super(self.ivars().picker.clone()),
        )))
    }
}

impl WorktreeCell {
    pub const IDENTIFIER: &'static str = "wtm.worktree";

    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let picker = PillButton::with_title(&ns(""), None, sel!(performClick:), mtm);
        picker.setBordered(false);
        picker.setControlSize(NSControlSize::Small);
        picker.setFont(Some(&crate::util::branch_font()));
        if let Some(chevrons) = symbol_raised(
            "chevron.up.chevron.down",
            "Switch branch",
            9.0,
            crate::util::SEMIBOLD,
            CHEVRON_GAP,
            CHEVRON_LIFT,
        ) {
            picker.setImage(Some(&chevrons));
        }
        picker.setImagePosition(NSCellImagePosition::ImageTrailing);
        picker.setImageHugsTitle(true);
        picker.setTranslatesAutoresizingMaskIntoConstraints(false);
        picker.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow + 10.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        picker.setToolTip(Some(&ns("Switch branch")));
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

        let copy_branch = icon_button(Icon::Copy, "Copy branch name", mtm);
        let copy_path = icon_button(Icon::Copy, "Copy path", mtm);
        let actions: Vec<_> = ACTIONS
            .iter()
            .map(|&(id, icon)| (id, icon_button(icon, "", mtm)))
            .collect();

        let key: RowKey = Rc::default();
        let mut targets = vec![
            on_press(
                &picker,
                &key,
                |k| controller::with_worktree(k, |w| w.on_switch.call(())),
                mtm,
            ),
            on_press(
                &copy_branch,
                &key,
                |k| {
                    controller::with_worktree(k, |w| {
                        if let Some(h) = &w.on_copy_branch {
                            h.call(())
                        }
                    })
                },
                mtm,
            ),
            on_press(
                &copy_path,
                &key,
                |k| controller::with_worktree(k, |w| w.on_copy_path.call(())),
                mtm,
            ),
        ];
        for (id, button) in &actions {
            let id = *id;
            targets.push(on_press(
                button,
                &key,
                move |k| {
                    controller::with_worktree(k, |w| {
                        if let Some(a) = w.action(id) {
                            a.on_press.call(())
                        }
                    })
                },
                mtm,
            ));
        }

        let this = mtm.alloc::<Self>().set_ivars(WorktreeCellIvars {
            key,
            branch: RefCell::new(Rich::default()),
            picker: picker.clone(),
            copy_branch: copy_branch.clone(),
            badges: badges.clone(),
            badge_views: RefCell::new(Vec::new()),
            spinner: spinner.clone(),
            busy: busy.clone(),
            path_label: path_label.clone(),
            actions: actions.clone(),
            danger: RefCell::new(false),
            top: RefCell::new(None),
            _targets: targets,
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(400.0, WORKTREE_ROW_HEIGHT))]
        };
        this.setIdentifier(Some(&ns(Self::IDENTIFIER)));

        let line1 = hstack(6.0, mtm);
        line1.addArrangedSubview(&picker);
        line1.addArrangedSubview(&copy_branch);
        line1.addArrangedSubview(&spacer(mtm));
        line1.addArrangedSubview(&spinner);
        line1.addArrangedSubview(&busy);
        line1.addArrangedSubview(&badges);
        let line2 = hstack(2.0, mtm);
        line2.addArrangedSubview(&path_label);
        line2.addArrangedSubview(&copy_path);
        line2.addArrangedSubview(&spacer(mtm));
        for (i, (_, b)) in actions.iter().enumerate() {
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
        *this.ivars().top.borrow_mut() = Some(pin(&this, &col, PLATE_INSETS));
        this.paint_delete_tint();
        this
    }

    pub fn configure(&self, key: &str, w: &WorktreeRow) {
        let iv = self.ivars();
        *iv.key.borrow_mut() = key.to_string();
        *iv.branch.borrow_mut() = w.branch.clone();
        iv.copy_branch.setHidden(w.on_copy_branch.is_none());

        // Badges: reuse existing views, add or drop the difference.
        let mut views = iv.badge_views.borrow_mut();
        let mtm = MainThreadMarker::from(self);
        while views.len() > w.badges.len() {
            let v = views.pop().unwrap();
            iv.badges.removeArrangedSubview(&v);
            v.removeFromSuperview();
        }
        for (i, b) in w.badges.iter().enumerate() {
            let tone = BadgeTone {
                hue: b.hue,
                emphasis: b.emphasis,
            };
            if i < views.len() {
                views[i].set(&b.text, tone, &b.tooltip);
            } else {
                let v = Badge::new(&b.text, tone, &b.tooltip, mtm);
                iv.badges.addArrangedSubview(&v);
                views.push(v);
            }
        }
        drop(views);

        iv.path_label.setStringValue(&ns(&w.path));
        iv.path_label.setToolTip(Some(&ns(&w.path_full)));

        match &w.busy {
            Some(b) => {
                unsafe { iv.spinner.startAnimation(None) };
                iv.busy.setStringValue(&ns(b));
                iv.busy.setHidden(false);
            }
            None => {
                unsafe { iv.spinner.stopAnimation(None) };
                iv.busy.setHidden(true);
            }
        }
        // The attributed title carries its own colour, so the disabled look
        // has to be chosen here rather than left to AppKit.
        iv.picker.setEnabled(w.can_switch);
        self.paint_branch_title();
        iv.picker.setToolTip(Some(&ns(&w.switch_hint)));
        // The title may draw an agent prefix as a mark, which would otherwise
        // drop those words from the name assistive technology reads out.
        iv.picker.setAccessibilityLabel(Some(&ns(&w.branch_name)));

        for (id, button) in &iv.actions {
            match w.action(id) {
                Some(a) => {
                    button.setEnabled(a.enabled);
                    button.setHidden(a.hidden);
                    button.set_hint(&a.hint);
                    if *id == "delete" {
                        *iv.danger.borrow_mut() = a.tint == Tint::Danger;
                    }
                }
                None => button.setHidden(true),
            }
        }
        self.paint_delete_tint();
    }
}

impl PlateCell for WorktreeCell {
    fn set_lead(&self, lead: f64) {
        if let Some(c) = self.ivars().top.borrow().as_ref() {
            c.setConstant(PLATE_INSETS.2 + lead);
        }
    }
}

// MARK: Pending creation row

pub struct PendingCellIvars {
    key: RowKey,
    top: RefCell<Option<Retained<NSLayoutConstraint>>>,
    branch: Retained<NSTextField>,
    spinner: Retained<NSProgressIndicator>,
    status: Retained<NSTextField>,
    dismiss: Retained<IconButton>,
    /// Its buttons' targets; a button holds its target weakly.
    _targets: Vec<Retained<Callback>>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMPendingCell"]
    #[ivars = PendingCellIvars]
    pub struct PendingCell;
);

impl PendingCell {
    pub const IDENTIFIER: &'static str = "wtm.pending";

    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let branch = label("", mtm);
        branch.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
            12.0,
            crate::util::SEMIBOLD,
        )));
        let spinner = small_spinner(mtm);
        let status = secondary_label("Creating…", 11.0, mtm);
        let dismiss = icon_button(Icon::Close, "Dismiss", mtm);
        let key: RowKey = Rc::default();
        let targets = vec![on_press(
            &dismiss,
            &key,
            |k| controller::with_pending(k, |p| p.on_dismiss.call(())),
            mtm,
        )];
        let this = mtm.alloc::<Self>().set_ivars(PendingCellIvars {
            key,
            top: RefCell::new(None),
            branch: branch.clone(),
            spinner: spinner.clone(),
            status: status.clone(),
            dismiss: dismiss.clone(),
            _targets: targets,
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(400.0, PENDING_ROW_HEIGHT))]
        };
        this.setIdentifier(Some(&ns(Self::IDENTIFIER)));
        let line = hstack(8.0, mtm);
        line.addArrangedSubview(&branch);
        line.addArrangedSubview(&spinner);
        line.addArrangedSubview(&status);
        line.addArrangedSubview(&spacer(mtm));
        line.addArrangedSubview(&dismiss);
        *this.ivars().top.borrow_mut() = Some(pin(&this, &line, PLATE_INSETS));
        this
    }

    pub fn configure(&self, key: &str, p: &PendingRow) {
        let iv = self.ivars();
        *iv.key.borrow_mut() = key.to_string();
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

impl PlateCell for PendingCell {
    fn set_lead(&self, lead: f64) {
        if let Some(c) = self.ivars().top.borrow().as_ref() {
            c.setConstant(PLATE_INSETS.2 + lead);
        }
    }
}
