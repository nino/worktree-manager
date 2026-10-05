//! The list's rows: a repo card's header, a worktree's plate and a creation
//! in flight. Each keeps its widgets and patches them from the next row of
//! the same key, so the keyboard, an open popover's anchor and a tooltip
//! under the pointer survive a status change.

use std::cell::RefCell;

use gtk::pango;
use gtk::prelude::*;

use wtm_toolkit::{Icon, Id, PendingRow, RepoHeader, RowAction, Tint, WorktreeRow, BRANCH_BUTTON};

use crate::draw;
use crate::elements::{badge, patch_badge};
use crate::rich::{LabelOpts, RichLabel};
use crate::util::{
    icon_button, label, on_click, put, set_hint, set_label_text, set_sensitive, set_tooltip, show,
    slot, spin, Slot,
};

fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

/// A row's root: an item of the list's tree, where its selected and
/// expanded states mean something to assistive technology. The role can
/// only be given when the widget is made.
fn tree_item(orientation: gtk::Orientation, spacing: i32, level: i32) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(orientation)
        .spacing(spacing)
        .accessible_role(gtk::AccessibleRole::TreeItem)
        .build();
    b.update_property(&[gtk::accessible::Property::Level(level)]);
    b
}

fn spacer() -> gtk::Box {
    let s = hbox(0);
    s.set_hexpand(true);
    s
}

/// A path: monospaced, secondary, and the first thing to give way (from the
/// middle, so both the root and the folder name stay readable).
fn path_label() -> gtk::Label {
    let l = label("", &["t-path", "ink-secondary"]);
    l.set_ellipsize(pango::EllipsizeMode::Middle);
    l.set_width_chars(6);
    l.set_selectable(false);
    l
}

// MARK: Repo header

pub struct HeaderW {
    pub root: gtk::Box,
    /// Opens and closes the card; wired by the list, which knows the key.
    pub disclosure: gtk::Button,
    name: gtk::Label,
    meta: gtk::Label,
    spinner: gtk::Spinner,
    path: gtk::Label,
    error: gtk::Label,
    new_worktree: gtk::Button,
    settings: gtk::Button,
    copy: gtk::Button,
    on_new: Slot<()>,
    on_settings: Slot<()>,
    on_copy: Slot<()>,
    last: RefCell<Option<RepoHeader>>,
}

impl HeaderW {
    pub fn new(h: &RepoHeader) -> HeaderW {
        let root = tree_item(gtk::Orientation::Horizontal, 8, 1);
        root.add_css_class("wtm-header");
        root.set_focusable(true);
        let disclosure = gtk::Button::from_icon_name("pan-down-symbolic");
        disclosure.add_css_class("flat");
        disclosure.add_css_class("wtm-disclosure");
        disclosure.set_valign(gtk::Align::Center);
        // The keyboard opens and closes cards with ← and → on the row; a
        // Tab stop of its own would only lengthen the loop.
        disclosure.set_focusable(false);

        let name = label("", &["t-title"]);
        name.set_ellipsize(pango::EllipsizeMode::End);
        name.set_width_chars(4);
        let meta = label("", &["t-caption", "ink-secondary"]);
        let spinner = gtk::Spinner::new();
        let line1 = hbox(8);
        line1.append(&name);
        line1.append(&meta);
        line1.append(&spinner);

        let path = path_label();
        let copy = icon_button(Icon::Copy, "Copy path");
        let error = label("", &["t-caption", "ink-error"]);
        error.set_ellipsize(pango::EllipsizeMode::End);
        error.set_width_chars(6);
        let line2 = hbox(4);
        line2.append(&path);
        line2.append(&copy);
        line2.append(&error);

        let text = vbox(1);
        text.append(&line1);
        text.append(&line2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);

        let new_worktree = gtk::Button::with_label("New Worktree");
        new_worktree.add_css_class("wtm-small");
        new_worktree.set_valign(gtk::Align::Center);
        let settings = icon_button(Icon::Settings, "Repo settings");

        root.append(&disclosure);
        root.append(&text);
        root.append(&new_worktree);
        root.append(&settings);

        let on_new = slot(&h.on_new_worktree);
        let on_settings = slot(&h.on_settings);
        let on_copy = slot(&h.on_copy_path);
        on_click(&new_worktree, &on_new);
        on_click(&settings, &on_settings);
        on_click(&copy, &on_copy);

        let w = HeaderW {
            root,
            disclosure,
            name,
            meta,
            spinner,
            path,
            error,
            new_worktree,
            settings,
            copy,
            on_new,
            on_settings,
            on_copy,
            last: RefCell::new(None),
        };
        w.patch(h);
        w
    }

    pub fn patch(&self, h: &RepoHeader) {
        put(&self.on_new, &h.on_new_worktree);
        put(&self.on_settings, &h.on_settings);
        put(&self.on_copy, &h.on_copy_path);
        if self.last.borrow().as_ref() == Some(h) {
            return;
        }
        set_label_text(&self.name, &h.name);
        set_label_text(&self.meta, &h.meta);
        spin(&self.spinner, h.loading);
        set_tooltip(&self.spinner, h.loading.then_some("Listing worktrees…"));
        set_label_text(&self.path, &h.path);
        set_tooltip(&self.path, Some(&h.path_full));
        match &h.error {
            Some(e) => {
                set_label_text(&self.error, e);
                set_tooltip(&self.error, Some(e));
                show(&self.error, true);
            }
            None => show(&self.error, false),
        }
        set_sensitive(&self.new_worktree, h.can_create);
        self.root
            .update_property(&[gtk::accessible::Property::Label(&h.name)]);
        *self.last.borrow_mut() = Some(h.clone());
    }

    pub fn set_expanded(&self, open: bool) {
        let icon = if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        };
        if self.disclosure.icon_name().as_deref() != Some(icon) {
            self.disclosure.set_icon_name(icon);
            set_hint(
                &self.disclosure,
                if open { "Close card" } else { "Open card" },
                true,
            );
        }
        self.root
            .update_state(&[gtk::accessible::State::Expanded(Some(open))]);
    }

    /// A button by the id the tests and the screenshot driver use.
    pub fn button(&self, id: &str) -> Option<gtk::Widget> {
        Some(match id {
            "new-worktree" => self.new_worktree.clone().upcast(),
            "settings" => self.settings.clone().upcast(),
            "copy-path" => self.copy.clone().upcast(),
            "disclosure" => self.disclosure.clone().upcast(),
            _ => return None,
        })
    }
}

// MARK: Worktree row

struct ActionW {
    id: Id,
    button: gtk::Button,
    on_press: Slot<()>,
    last: RowAction,
}

pub struct WorktreeW {
    pub root: gtk::Box,
    pub branch: gtk::Button,
    branch_label: RichLabel,
    copy_branch: gtk::Button,
    spinner: gtk::Spinner,
    busy: gtk::Label,
    badges: gtk::Box,
    badge_labels: RefCell<Vec<gtk::Label>>,
    path: gtk::Label,
    copy_path: gtk::Button,
    actions_box: gtk::Box,
    actions: RefCell<Vec<ActionW>>,
    action_shape: RefCell<Vec<Vec<Id>>>,
    on_switch: Slot<()>,
    on_copy_branch: Slot<()>,
    on_copy_path: Slot<()>,
    last: RefCell<Option<WorktreeRow>>,
}

impl WorktreeW {
    pub fn new(w: &WorktreeRow) -> WorktreeW {
        let root = tree_item(gtk::Orientation::Vertical, 3, 2);
        root.add_css_class("wtm-plate");
        root.set_focusable(true);

        let branch_label = RichLabel::new(
            &w.branch,
            LabelOpts {
                ellipsize: true,
                ..LabelOpts::default()
            },
        );
        branch_label.root.add_css_class("t-branch");
        let content = hbox(3);
        content.append(&branch_label.root);
        content.append(&draw::chevrons());
        let branch = gtk::Button::new();
        branch.set_child(Some(&content));
        branch.add_css_class("wtm-pill");
        branch.set_valign(gtk::Align::Center);
        let copy_branch = icon_button(Icon::Copy, "Copy branch name");
        let spinner = gtk::Spinner::new();
        let busy = label("", &["t-caption", "ink-secondary"]);
        let badges = hbox(4);
        badges.set_valign(gtk::Align::Center);
        let line1 = hbox(6);
        line1.append(&branch);
        line1.append(&copy_branch);
        line1.append(&spacer());
        line1.append(&spinner);
        line1.append(&busy);
        line1.append(&badges);

        let path = path_label();
        let copy_path = icon_button(Icon::Copy, "Copy path");
        let actions_box = hbox(0);
        let line2 = hbox(2);
        line2.append(&path);
        line2.append(&copy_path);
        line2.append(&spacer());
        line2.append(&actions_box);

        root.append(&line1);
        root.append(&line2);

        let on_switch = slot(&w.on_switch);
        let on_copy_branch = slot(&w.on_copy_branch.clone().unwrap_or_default());
        let on_copy_path = slot(&w.on_copy_path);
        on_click(&copy_branch, &on_copy_branch);
        on_click(&copy_path, &on_copy_path);
        {
            let s = on_switch.clone();
            let me = branch.clone();
            branch.connect_clicked(move |_| {
                // The click that dismissed this button's own popover is not
                // a new request to open it.
                if crate::popover::just_dismissed_from(&me) {
                    return;
                }
                crate::util::fire(&s, ());
            });
        }

        let row = WorktreeW {
            root,
            branch,
            branch_label,
            copy_branch,
            spinner,
            busy,
            badges,
            badge_labels: RefCell::new(Vec::new()),
            path,
            copy_path,
            actions_box,
            actions: RefCell::new(Vec::new()),
            action_shape: RefCell::new(Vec::new()),
            on_switch,
            on_copy_branch,
            on_copy_path,
            last: RefCell::new(None),
        };
        row.patch(w);
        row
    }

    pub fn patch(&self, w: &WorktreeRow) {
        put(&self.on_switch, &w.on_switch);
        if let Some(h) = &w.on_copy_branch {
            put(&self.on_copy_branch, h);
        }
        put(&self.on_copy_path, &w.on_copy_path);
        self.patch_actions(&w.actions);
        let last = self.last.borrow().clone();
        if last.as_ref() == Some(w) {
            return;
        }
        if last.as_ref().map(|l| &l.branch) != Some(&w.branch) {
            self.branch_label.set(&w.branch);
        }
        set_sensitive(&self.branch, w.can_switch);
        set_tooltip(&self.branch, Some(&w.switch_hint));
        // The mark stands in for a prefix: the whole name is what is read
        // out, and what the tooltip says.
        self.branch.update_property(&[
            gtk::accessible::Property::Label(&w.branch_name),
            gtk::accessible::Property::Description(&w.switch_hint),
        ]);
        show(&self.copy_branch, w.on_copy_branch.is_some());
        self.patch_badges(w);
        spin(&self.spinner, w.busy.is_some());
        match &w.busy {
            Some(b) => {
                set_label_text(&self.busy, b);
                show(&self.busy, true);
            }
            None => show(&self.busy, false),
        }
        set_label_text(&self.path, &w.path);
        set_tooltip(&self.path, Some(&w.path_full));
        self.root
            .update_property(&[gtk::accessible::Property::Label(&w.branch_name)]);
        *self.last.borrow_mut() = Some(w.clone());
    }

    /// Badges reuse their labels by position: most status changes swap one
    /// badge's text and colour.
    fn patch_badges(&self, w: &WorktreeRow) {
        let old: Vec<wtm_toolkit::Badge> = self
            .last
            .borrow()
            .as_ref()
            .map(|l| l.badges.clone())
            .unwrap_or_default();
        let mut labels = self.badge_labels.borrow_mut();
        while labels.len() > w.badges.len() {
            let l = labels.pop().expect("checked");
            self.badges.remove(&l);
        }
        for (i, b) in w.badges.iter().enumerate() {
            if i < labels.len() {
                patch_badge(&labels[i], old.get(i), b);
            } else {
                let l = badge(b);
                self.badges.append(&l);
                labels.push(l);
            }
        }
    }

    fn patch_actions(&self, groups: &[Vec<RowAction>]) {
        let shape: Vec<Vec<Id>> = groups
            .iter()
            .map(|g| g.iter().map(|a| a.id).collect())
            .collect();
        if *self.action_shape.borrow() != shape {
            while let Some(c) = self.actions_box.first_child() {
                self.actions_box.remove(&c);
            }
            let mut actions = Vec::new();
            for (gi, group) in groups.iter().enumerate() {
                for (i, a) in group.iter().enumerate() {
                    let b = icon_button(a.icon, &a.hint);
                    // A wider gap between groups than within one.
                    b.set_margin_start(if gi > 0 && i == 0 { 10 } else { 2 });
                    let on_press = slot(&a.on_press);
                    on_click(&b, &on_press);
                    self.actions_box.append(&b);
                    let aw = ActionW {
                        id: a.id,
                        button: b,
                        on_press,
                        last: a.clone(),
                    };
                    apply_action(&aw.button, None, a);
                    actions.push(aw);
                }
            }
            *self.actions.borrow_mut() = actions;
            *self.action_shape.borrow_mut() = shape;
            return;
        }
        let mut actions = self.actions.borrow_mut();
        for (aw, a) in actions.iter_mut().zip(groups.iter().flatten()) {
            put(&aw.on_press, &a.on_press);
            if &aw.last != a {
                apply_action(&aw.button, Some(&aw.last), a);
                aw.last = a.clone();
            }
        }
    }

    /// A button by id: the branch button, the copy buttons, or an action.
    pub fn button(&self, id: &str) -> Option<gtk::Widget> {
        match id {
            BRANCH_BUTTON => Some(self.branch.clone().upcast()),
            "copy-branch" => Some(self.copy_branch.clone().upcast()),
            "copy-path" => Some(self.copy_path.clone().upcast()),
            _ => self
                .actions
                .borrow()
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.button.clone().upcast()),
        }
    }

    /// The badge labels, in order (tests).
    pub fn badge_texts(&self) -> Vec<String> {
        self.badge_labels
            .borrow()
            .iter()
            .map(|l| l.text().to_string())
            .collect()
    }
}

fn apply_action(b: &gtk::Button, old: Option<&RowAction>, a: &RowAction) {
    set_sensitive(b, a.enabled);
    show(b, !a.hidden);
    if old.map(|o| o.hint.as_str()) != Some(a.hint.as_str()) {
        set_hint(b, &a.hint, true);
    }
    if a.tint == Tint::Danger {
        b.add_css_class("danger");
    } else {
        b.remove_css_class("danger");
    }
}

// MARK: Pending creation

pub struct PendingW {
    pub root: gtk::Box,
    branch: gtk::Label,
    spinner: gtk::Spinner,
    status: gtk::Label,
    dismiss: gtk::Button,
    on_dismiss: Slot<()>,
    last: RefCell<Option<PendingRow>>,
}

impl PendingW {
    pub fn new(p: &PendingRow) -> PendingW {
        let root = tree_item(gtk::Orientation::Horizontal, 8, 2);
        root.add_css_class("wtm-plate");
        root.set_focusable(true);
        let branch = label("", &["t-branch-strong"]);
        branch.set_ellipsize(pango::EllipsizeMode::End);
        branch.set_width_chars(6);
        let spinner = gtk::Spinner::new();
        let status = label("", &["t-caption"]);
        status.set_ellipsize(pango::EllipsizeMode::End);
        status.set_width_chars(6);
        let dismiss = icon_button(Icon::Close, "Dismiss");
        root.append(&branch);
        root.append(&spinner);
        root.append(&status);
        root.append(&spacer());
        root.append(&dismiss);
        let on_dismiss = slot(&p.on_dismiss);
        on_click(&dismiss, &on_dismiss);
        let w = PendingW {
            root,
            branch,
            spinner,
            status,
            dismiss,
            on_dismiss,
            last: RefCell::new(None),
        };
        w.patch(p);
        w
    }

    pub fn patch(&self, p: &PendingRow) {
        put(&self.on_dismiss, &p.on_dismiss);
        if self.last.borrow().as_ref() == Some(p) {
            return;
        }
        set_label_text(&self.branch, &p.branch);
        match &p.error {
            Some(e) => {
                spin(&self.spinner, false);
                set_label_text(&self.status, e);
                set_tooltip(&self.status, Some(e));
                self.status.remove_css_class("ink-secondary");
                self.status.add_css_class("ink-error");
                show(&self.dismiss, true);
            }
            None => {
                spin(&self.spinner, true);
                set_label_text(&self.status, "Creating…");
                set_tooltip(&self.status, None);
                self.status.remove_css_class("ink-error");
                self.status.add_css_class("ink-secondary");
                show(&self.dismiss, false);
            }
        }
        *self.last.borrow_mut() = Some(p.clone());
    }

    pub fn button(&self, id: &str) -> Option<gtk::Widget> {
        (id == "dismiss").then(|| self.dismiss.clone().upcast())
    }
}
