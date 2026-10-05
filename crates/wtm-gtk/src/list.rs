//! The sectioned list: one card per repo, its header over a well of plates.
//!
//! A plain vertical box in a scrolled window rather than a `GtkListView`:
//! the list holds tens of rows, not thousands, and a box keeps one widget per
//! key that render patches in place, which a recycling view would not. The
//! selection, the keyboard (arrows, ← → to close and open a card, Space),
//! dragging a card to a new place and the card drawing are all here.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib};

use wtm_toolkit::{
    host_event, Handler, HostEvent, Key, Reorder, Row, RowContent, Section, TreeList,
};

use crate::rows::{HeaderW, PendingW, WorktreeW};
use crate::util::{fire, put, quietly, rendering, slot, Slot};

/// How long a card takes to open or close.
const REVEAL_MS: u32 = 150;

pub struct ListW {
    pub scroller: gtk::ScrolledWindow,
    pub content: gtk::Box,
    sections: RefCell<Vec<SectionW>>,
    /// The selection as shown: the view's, or the user's ahead of it.
    selected: RefCell<Option<Key>>,
    /// Keys of the rows on screen, in order, as last rendered.
    visible: RefCell<Vec<Key>>,
    on_select: Slot<Option<Key>>,
    on_toggle: Slot<(Key, bool)>,
    on_reorder: RefCell<Option<Handler<Reorder>>>,
    on_activate: Slot<Key>,
    /// A scroll offset waiting for the rows to be laid out (see `scroll_to`).
    pending_scroll: Cell<Option<f64>>,
    /// The give-up timer for `pending_scroll` has started.
    pending_timer: Cell<bool>,
    /// An `apply_pending_scroll` is queued for after this layout.
    apply_queued: Cell<bool>,
    /// The last offset reported as the user's (tests).
    reported: Cell<Option<f64>>,
    dragging: RefCell<Option<Key>>,
    drop_mark: RefCell<Option<gtk::Widget>>,
    me: Weak<ListW>,
}

struct SectionW {
    key: Key,
    card: gtk::Box,
    header: HeaderW,
    revealer: gtk::Revealer,
    well: gtk::Box,
    rows: Vec<RowW>,
    expanded: bool,
}

struct RowW {
    key: Key,
    kind: RowKind,
}

enum RowKind {
    // Boxed: a worktree row holds far more widgets than a placeholder.
    Worktree(Box<WorktreeW>),
    Pending(PendingW),
}

impl RowW {
    fn root(&self) -> gtk::Widget {
        match &self.kind {
            RowKind::Worktree(w) => w.root.clone().upcast(),
            RowKind::Pending(p) => p.root.clone().upcast(),
        }
    }
}

impl ListW {
    pub fn new() -> Rc<ListW> {
        Rc::new_cyclic(|me: &Weak<ListW>| {
            let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
            content.add_css_class("wtm-list");
            // With nothing selected the list itself holds the keyboard, so
            // the arrow keys work straight away.
            content.set_focusable(true);
            // An explicit viewport, to turn off its scroll-to-focus: that
            // scrolled on every focus change, the backend's own included
            // (the restored row at launch would undo the restored offset).
            // A row the user moves the keyboard to is scrolled into view by
            // `wire_row` instead.
            let viewport = gtk::Viewport::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
            viewport.set_scroll_to_focus(false);
            viewport.set_child(Some(&content));
            let scroller = gtk::ScrolledWindow::new();
            scroller.set_child(Some(&viewport));
            scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
            scroller.set_vexpand(true);
            scroller.set_hexpand(true);
            ListW {
                scroller,
                content,
                sections: RefCell::new(Vec::new()),
                selected: RefCell::new(None),
                visible: RefCell::new(Vec::new()),
                on_select: slot(&Handler::none()),
                on_toggle: slot(&Handler::none()),
                on_reorder: RefCell::new(None),
                on_activate: slot(&Handler::none()),
                pending_scroll: Cell::new(None),
                pending_timer: Cell::new(false),
                apply_queued: Cell::new(false),
                reported: Cell::new(None),
                dragging: RefCell::new(None),
                drop_mark: RefCell::new(None),
                me: me.clone(),
            }
        })
        .wired()
    }

    fn wired(self: Rc<Self>) -> Rc<Self> {
        let keys = gtk::EventControllerKey::new();
        let me = self.me.clone();
        keys.connect_key_pressed(move |_, key, _, mods| match me.upgrade() {
            Some(list) => list.key(key, mods),
            None => glib::Propagation::Proceed,
        });
        self.scroller.add_controller(keys);

        // What the user does in the list before a restored offset lands
        // makes it theirs: a wheel or touchpad, a press (the scrollbar, a
        // row, a touchscreen), a key. Seen in the capture phase, before the
        // scrolled window acts on it.
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = self.me.clone();
        wheel.connect_scroll(move |_, _, _| {
            if let Some(list) = me.upgrade() {
                list.user_input();
            }
            glib::Propagation::Proceed
        });
        self.scroller.add_controller(wheel);
        let press = gtk::GestureClick::new();
        press.set_button(0);
        press.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = self.me.clone();
        press.connect_pressed(move |_, _, _, _| {
            if let Some(list) = me.upgrade() {
                list.user_input();
            }
        });
        self.scroller.add_controller(press);
        let any_key = gtk::EventControllerKey::new();
        any_key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = self.me.clone();
        any_key.connect_key_pressed(move |_, _, _, _| {
            if let Some(list) = me.upgrade() {
                list.user_input();
            }
            glib::Propagation::Proceed
        });
        self.scroller.add_controller(any_key);

        let adj = self.scroller.vadjustment();
        let me = self.me.clone();
        adj.connect_value_changed(move |a| {
            if rendering() {
                return;
            }
            let Some(list) = me.upgrade() else { return };
            // Until a restored offset lands, a move is GTK's own: the range
            // clamping the value while the rows are laid out. Reported, it
            // would be remembered in place of the offset being restored.
            if list.pending_scroll.get().is_some() {
                return;
            }
            list.reported.set(Some(a.value()));
            host_event(HostEvent::Scrolled(a.value()));
        });
        let me = self.me.clone();
        adj.connect_changed(move |_| {
            if let Some(list) = me.upgrade() {
                list.queue_pending_scroll();
            }
        });

        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        let me = self.me.clone();
        drop.connect_motion(move |_, _, y| {
            if let Some(list) = me.upgrade() {
                list.show_drop(y);
            }
            gdk::DragAction::MOVE
        });
        let me = self.me.clone();
        drop.connect_leave(move |_| {
            if let Some(list) = me.upgrade() {
                list.clear_drop();
            }
        });
        let me = self.me.clone();
        drop.connect_drop(move |_, value, _, y| {
            let Some(list) = me.upgrade() else {
                return false;
            };
            list.clear_drop();
            let Ok(key) = value.get::<String>() else {
                return false;
            };
            list.dropped(key, y)
        });
        self.content.add_controller(drop);
        self
    }

    // MARK: Rendering

    pub fn render(&self, view: &TreeList) {
        put(&self.on_select, &view.on_select);
        put(&self.on_toggle, &view.on_toggle);
        put(&self.on_activate, &view.on_activate);
        *self.on_reorder.borrow_mut() = view.on_reorder.clone();

        let mut old: HashMap<Key, SectionW> = self
            .sections
            .borrow_mut()
            .drain(..)
            .map(|s| (s.key.clone(), s))
            .collect();
        let mut sections = Vec::with_capacity(view.sections.len());
        for s in &view.sections {
            let sw = match old.remove(&s.key) {
                Some(mut sw) => {
                    sw.patch(s, &self.me);
                    sw
                }
                None => SectionW::new(s, &self.me),
            };
            sections.push(sw);
        }
        for (_, gone) in old {
            self.content.remove(&gone.card);
        }
        let mut prev: Option<gtk::Widget> = None;
        for sw in &sections {
            let card: gtk::Widget = sw.card.clone().upcast();
            if card.parent().is_none() {
                self.content.insert_child_after(&card, prev.as_ref());
            } else if card.prev_sibling() != prev {
                self.content.reorder_child_after(&card, prev.as_ref());
            }
            prev = Some(card);
        }
        *self.sections.borrow_mut() = sections;
        *self.visible.borrow_mut() = view.visible_keys().into_iter().map(String::from).collect();

        // The view's selection, without reporting it back. Re-marked every
        // render: a row made this render starts unmarked.
        let shown = self.selected.borrow().clone();
        if let Some(k) = &shown {
            self.mark(k, false);
        }
        *self.selected.borrow_mut() = view.selected.clone();
        if let Some(k) = &view.selected {
            self.mark(k, true);
        }
    }

    fn mark(&self, key: &str, on: bool) {
        if let Some(w) = self.widget(key) {
            if on {
                w.add_css_class("selected");
            } else {
                w.remove_css_class("selected");
            }
            w.update_state(&[gtk::accessible::State::Selected(Some(on))]);
        }
    }

    /// The row widget for a section's or a row's key.
    pub fn widget(&self, key: &str) -> Option<gtk::Widget> {
        let sections = self.sections.borrow();
        for s in sections.iter() {
            if s.key == key {
                return Some(s.header.root.clone().upcast());
            }
            if let Some(r) = s.rows.iter().find(|r| r.key == key) {
                return Some(r.root());
            }
        }
        None
    }

    /// An element of a row: a popover's anchor, or a button for the tests.
    pub fn element(&self, key: &str, id: &str) -> Option<gtk::Widget> {
        let sections = self.sections.borrow();
        for s in sections.iter() {
            if s.key == key {
                return s.header.button(id);
            }
            if let Some(r) = s.rows.iter().find(|r| r.key == key) {
                return match &r.kind {
                    RowKind::Worktree(w) => w.button(id),
                    RowKind::Pending(p) => p.button(id),
                };
            }
        }
        None
    }

    /// The badge texts of a worktree row (tests).
    pub fn badges(&self, key: &str) -> Vec<String> {
        let sections = self.sections.borrow();
        sections
            .iter()
            .flat_map(|s| s.rows.iter())
            .find(|r| r.key == key)
            .and_then(|r| match &r.kind {
                RowKind::Worktree(w) => Some(w.badge_texts()),
                RowKind::Pending(_) => None,
            })
            .unwrap_or_default()
    }

    /// Whether every card has finished opening or closing.
    pub fn settled(&self) -> bool {
        self.sections
            .borrow()
            .iter()
            .all(|s| s.revealer.is_child_revealed() == s.revealer.reveals_child())
    }

    // MARK: The user's changes

    /// The user put the cursor on `key`: shown at once, then reported.
    fn user_select(&self, key: &str) {
        if rendering() || self.selected.borrow().as_deref() == Some(key) {
            return;
        }
        let old = self.selected.borrow().clone();
        if let Some(k) = old {
            self.mark(&k, false);
        }
        *self.selected.borrow_mut() = Some(key.to_string());
        self.mark(key, true);
        fire(&self.on_select, Some(key.to_string()));
    }

    /// The disclosure was clicked: ask for the other state, and let the view
    /// decide.
    fn user_toggle(&self, key: &str) {
        let open = self
            .sections
            .borrow()
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.expanded);
        if let Some(open) = open {
            fire(&self.on_toggle, (key.to_string(), !open));
        }
    }

    fn focus_key(&self, key: &str) {
        if let Some(w) = self.widget(key) {
            w.grab_focus();
            // Focus selects; a widget that would not take it still moves the
            // cursor.
            self.user_select(key);
        }
    }

    fn key(&self, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        if mods.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        let visible = self.visible.borrow().clone();
        let selected = self.selected.borrow().clone();
        let at = selected
            .as_ref()
            .and_then(|k| visible.iter().position(|v| v == k));
        let section = |k: &str| {
            self.sections
                .borrow()
                .iter()
                .find(|s| s.key == k)
                .map(|s| (s.expanded, s.rows.first().map(|r| r.key.clone())))
        };
        let owner = |k: &str| {
            self.sections
                .borrow()
                .iter()
                .find(|s| s.rows.iter().any(|r| r.key == k))
                .map(|s| s.key.clone())
        };
        let target = match key {
            gdk::Key::Down | gdk::Key::KP_Down => match at {
                Some(i) => visible.get(i + 1).cloned(),
                None => visible.first().cloned(),
            },
            gdk::Key::Up | gdk::Key::KP_Up => match at {
                Some(i) => i.checked_sub(1).and_then(|i| visible.get(i).cloned()),
                None => visible.last().cloned(),
            },
            gdk::Key::Home | gdk::Key::KP_Home => visible.first().cloned(),
            gdk::Key::End | gdk::Key::KP_End => visible.last().cloned(),
            gdk::Key::Left | gdk::Key::KP_Left => {
                let Some(k) = selected else {
                    return glib::Propagation::Proceed;
                };
                match section(&k) {
                    Some((true, _)) => {
                        fire(&self.on_toggle, (k, false));
                        return glib::Propagation::Stop;
                    }
                    Some((false, _)) => return glib::Propagation::Stop,
                    None => owner(&k),
                }
            }
            gdk::Key::Right | gdk::Key::KP_Right => {
                let Some(k) = selected else {
                    return glib::Propagation::Proceed;
                };
                match section(&k) {
                    Some((false, _)) => {
                        fire(&self.on_toggle, (k, true));
                        return glib::Propagation::Stop;
                    }
                    Some((true, first)) => first,
                    None => return glib::Propagation::Proceed,
                }
            }
            gdk::Key::space | gdk::Key::KP_Space => {
                // A button with the keyboard takes Space itself, before this.
                if let Some(k) = selected {
                    fire(&self.on_activate, k);
                }
                return glib::Propagation::Stop;
            }
            _ => return glib::Propagation::Proceed,
        };
        if let Some(t) = target {
            self.focus_key(&t);
        }
        glib::Propagation::Stop
    }

    // MARK: Dragging a card

    fn sections_y(&self) -> Vec<(Key, f32, f32)> {
        self.sections
            .borrow()
            .iter()
            .filter_map(|s| {
                let b = s.card.compute_bounds(&self.content)?;
                Some((s.key.clone(), b.y(), b.height()))
            })
            .collect()
    }

    /// The section a drop at `y` lands before, `None` for the end.
    fn before_at(&self, y: f64) -> Option<Key> {
        self.sections_y()
            .into_iter()
            .find(|(_, top, h)| y < (*top + *h / 2.0) as f64)
            .map(|(k, _, _)| k)
    }

    fn show_drop(&self, y: f64) {
        let before = self.before_at(y);
        let target = match &before {
            Some(k) => self
                .sections
                .borrow()
                .iter()
                .find(|s| &s.key == k)
                .map(|s| s.card.clone().upcast()),
            None => None,
        };
        let current = self.drop_mark.borrow().clone();
        if current != target {
            self.clear_drop();
            if let Some(t) = &target {
                t.add_css_class("wtm-drop-before");
            }
            *self.drop_mark.borrow_mut() = target;
        }
    }

    fn clear_drop(&self) {
        if let Some(w) = self.drop_mark.borrow_mut().take() {
            w.remove_css_class("wtm-drop-before");
        }
    }

    /// Report a drop, unless it lands beside the dragged card itself, where
    /// nothing would move.
    fn dropped(&self, key: String, y: f64) -> bool {
        self.dragging.borrow_mut().take();
        let Some(handler) = self.on_reorder.borrow().clone() else {
            return false;
        };
        let order: Vec<Key> = self
            .sections
            .borrow()
            .iter()
            .map(|s| s.key.clone())
            .collect();
        let Some(from) = order.iter().position(|k| *k == key) else {
            return false;
        };
        let before = self.before_at(y);
        let next = order.get(from + 1).cloned();
        if before.as_ref() == Some(&key) || before == next {
            return false;
        }
        handler.call((key, before));
        true
    }

    // MARK: Effects

    /// Scroll to `offset` once the rows are laid out. It usually comes with
    /// the render that made the window, before any layout: the adjustment's
    /// range is only known after it, so the offset waits for the range to
    /// reach it, and settles for the clamped value a moment after the first
    /// layout when the list has become shorter than it was.
    pub fn scroll_to(&self, offset: f64) {
        self.pending_scroll.set(Some(offset));
        self.pending_timer.set(false);
        self.apply_pending_scroll();
    }

    /// The user scrolled, pressed or typed in the list: a restore not yet
    /// landed is dropped, and moves are theirs again.
    fn user_input(&self) {
        self.pending_scroll.set(None);
    }

    /// The range changed, which it does in the middle of a layout: apply
    /// the offset once that layout is done.
    fn queue_pending_scroll(&self) {
        if self.pending_scroll.get().is_none() || self.apply_queued.replace(true) {
            return;
        }
        let me = self.me.clone();
        glib::idle_add_local_once(move || {
            if let Some(list) = me.upgrade() {
                list.apply_queued.set(false);
                list.apply_pending_scroll();
            }
        });
    }

    fn apply_pending_scroll(&self) {
        let Some(offset) = self.pending_scroll.get() else {
            return;
        };
        let adj = self.scroller.vadjustment();
        // Not laid out yet: nothing to clamp against.
        if adj.page_size() <= 0.0 {
            return;
        }
        let max = (adj.upper() - adj.page_size()).max(0.0);
        let v = offset.clamp(0.0, max);
        if (adj.value() - v).abs() > 0.5 {
            quietly(|| adj.set_value(v));
        }
        if max >= offset {
            self.pending_scroll.set(None);
        } else if !self.pending_timer.replace(true) {
            let me = self.me.clone();
            glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                if let Some(list) = me.upgrade() {
                    list.pending_scroll.set(None);
                }
            });
        }
    }

    /// The offset last reported as the user's scroll (tests).
    pub fn reported_scroll(&self) -> Option<f64> {
        self.reported.get()
    }

    /// Scroll the selected row into view, after this render's layout.
    pub fn reveal_selection(&self) {
        let me = self.me.clone();
        self.scroller.add_tick_callback(move |_, _| {
            if let Some(list) = me.upgrade() {
                list.reveal_now();
            }
            glib::ControlFlow::Break
        });
    }

    fn reveal_now(&self) {
        let Some(key) = self.selected.borrow().clone() else {
            return;
        };
        let Some(w) = self.widget(&key) else { return };
        // A header reveals its whole card.
        let target = if w.has_css_class("wtm-header") {
            w.parent().unwrap_or(w)
        } else {
            w
        };
        self.reveal_widget(&target, 12.0);
    }

    /// Scroll the least that shows `w` (a row, or something in one) with
    /// `margin` around it. `false` when it is not laid out in the list.
    pub fn reveal_widget(&self, w: &gtk::Widget, margin: f64) -> bool {
        let Some(b) = w.compute_bounds(&self.content) else {
            return false;
        };
        let adj = self.scroller.vadjustment();
        let (top, bottom) = (b.y() as f64 - margin, (b.y() + b.height()) as f64 + margin);
        if top < adj.value() {
            adj.set_value(top.max(0.0));
        } else if bottom > adj.value() + adj.page_size() {
            adj.set_value(bottom - adj.page_size());
        }
        true
    }

    pub fn weak(&self) -> Weak<ListW> {
        self.me.clone()
    }

    /// Whether `w` is laid out and wholly inside the list's visible part.
    pub fn shows(&self, w: &gtk::Widget) -> bool {
        let Some(b) = w.compute_bounds(&self.content) else {
            return false;
        };
        let adj = self.scroller.vadjustment();
        w.is_mapped()
            && b.y() as f64 >= adj.value() - 0.5
            && (b.y() + b.height()) as f64 <= adj.value() + adj.page_size() + 0.5
    }

    /// Put the keyboard on the list: on the selected row, else the list.
    pub fn focus(&self) {
        let key = self.selected.borrow().clone();
        match key.and_then(|k| self.widget(&k)) {
            Some(w) => {
                w.grab_focus();
            }
            None => {
                self.content.grab_focus();
            }
        }
    }
}

// MARK: Sections and rows

/// Selection on click and on focus, and the header's drag, for a row's root.
/// A row the keyboard enters by the user's hand (arrows, Tab, a click) is
/// scrolled into view, which the viewport no longer does by itself.
fn wire_row(root: &gtk::Widget, key: &str, list: &Weak<ListW>) {
    let focus = gtk::EventControllerFocus::new();
    let (k, l, r) = (key.to_string(), list.clone(), root.downgrade());
    focus.connect_enter(move |_| {
        if let Some(list) = l.upgrade() {
            list.user_select(&k);
            if let (false, Some(root)) = (rendering(), r.upgrade()) {
                list.reveal_widget(&root, 0.0);
            }
        }
    });
    root.add_controller(focus);

    let click = gtk::GestureClick::new();
    // Capture: a click on a button in the row selects the row too.
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (k, l, r) = (key.to_string(), list.clone(), root.downgrade());
    click.connect_pressed(move |_, _, _, _| {
        if let (Some(list), Some(root)) = (l.upgrade(), r.upgrade()) {
            if !crate::util::has_keyboard(&root) {
                root.grab_focus();
            }
            list.user_select(&k);
        }
    });
    root.add_controller(click);
}

impl SectionW {
    fn new(s: &Section, list: &Weak<ListW>) -> SectionW {
        let header = HeaderW::new(&s.header);
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("wtm-card");
        // The card's rounded corners clip the header band and the well.
        card.set_overflow(gtk::Overflow::Hidden);
        let well = gtk::Box::new(gtk::Orientation::Vertical, 0);
        well.add_css_class("wtm-well");
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(REVEAL_MS);
        revealer.set_child(Some(&well));
        card.append(&header.root);
        card.append(&revealer);

        wire_row(header.root.upcast_ref(), &s.key, list);
        let (k, l) = (s.key.clone(), list.clone());
        header.disclosure.connect_clicked(move |_| {
            if let Some(list) = l.upgrade() {
                list.user_toggle(&k);
            }
        });
        wire_drag(&header.root, &card, &s.key, list);

        let mut sw = SectionW {
            key: s.key.clone(),
            card,
            header,
            revealer,
            well,
            rows: Vec::new(),
            expanded: s.expanded,
        };
        sw.patch_rows(&s.rows, list);
        // A card made open is shown open, not animated open.
        let show = s.expanded && !s.rows.is_empty();
        sw.revealer.set_transition_duration(0);
        sw.revealer.set_reveal_child(show);
        sw.revealer.set_transition_duration(REVEAL_MS);
        sw.header.set_expanded(s.expanded);
        sw
    }

    fn patch(&mut self, s: &Section, list: &Weak<ListW>) {
        self.header.patch(&s.header);
        self.patch_rows(&s.rows, list);
        self.expanded = s.expanded;
        self.header.set_expanded(s.expanded);
        let show = s.expanded && !s.rows.is_empty();
        if self.revealer.reveals_child() != show {
            self.revealer.set_reveal_child(show);
        }
    }

    fn patch_rows(&mut self, rows: &[Row], list: &Weak<ListW>) {
        let mut old: HashMap<Key, RowW> = self.rows.drain(..).map(|r| (r.key.clone(), r)).collect();
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let reused = old.remove(&r.key).and_then(|rw| {
                let ok = match (&rw.kind, &r.content) {
                    (RowKind::Worktree(w), RowContent::Worktree(v)) => {
                        w.patch(v);
                        true
                    }
                    (RowKind::Pending(p), RowContent::Pending(v)) => {
                        p.patch(v);
                        true
                    }
                    _ => false,
                };
                if ok {
                    Some(rw)
                } else {
                    self.well.remove(&rw.root());
                    None
                }
            });
            let rw = reused.unwrap_or_else(|| {
                let kind = match &r.content {
                    RowContent::Worktree(w) => RowKind::Worktree(Box::new(WorktreeW::new(w))),
                    RowContent::Pending(p) => RowKind::Pending(PendingW::new(p)),
                };
                let rw = RowW {
                    key: r.key.clone(),
                    kind,
                };
                wire_row(&rw.root(), &r.key, list);
                rw
            });
            out.push(rw);
        }
        for (_, gone) in old {
            self.well.remove(&gone.root());
        }
        let mut prev: Option<gtk::Widget> = None;
        for rw in &out {
            let root = rw.root();
            if root.parent().is_none() {
                self.well.insert_child_after(&root, prev.as_ref());
            } else if root.prev_sibling() != prev {
                self.well.reorder_child_after(&root, prev.as_ref());
            }
            prev = Some(root);
        }
        self.rows = out;
    }
}

/// A header drags its whole card to a new place, when the view allows it.
fn wire_drag(header: &gtk::Box, card: &gtk::Box, key: &str, list: &Weak<ListW>) {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let (k, l) = (key.to_string(), list.clone());
    source.connect_prepare(move |_, _, _| {
        let list = l.upgrade()?;
        list.on_reorder.borrow().as_ref()?;
        *list.dragging.borrow_mut() = Some(k.clone());
        Some(gdk::ContentProvider::for_value(&k.to_value()))
    });
    let c = card.downgrade();
    source.connect_drag_begin(move |source, _| {
        if let Some(card) = c.upgrade() {
            let icon = gtk::WidgetPaintable::new(Some(&card));
            source.set_icon(Some(&icon), 24, 16);
        }
    });
    let l = list.clone();
    source.connect_drag_end(move |_, _, _| {
        if let Some(list) = l.upgrade() {
            list.dragging.borrow_mut().take();
            list.clear_drop();
        }
    });
    header.add_controller(source);
}
