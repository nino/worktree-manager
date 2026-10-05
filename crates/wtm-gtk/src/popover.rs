//! The branch picker: a filter field over the list it narrows, in a popover
//! hung from a row's branch button. The field keeps the keyboard: ↑ and ↓
//! move the selection, Return chooses, Escape (or a click elsewhere) closes,
//! and only a close the user made is reported.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, glib};

use wtm_toolkit::{FilterList, ListItem, Popover};

use crate::list::ListW;
use crate::rich::{LabelOpts, RichLabel};
use crate::util::{fire, put, quietly, rendering, slot, Slot};

/// A click on the anchor this soon after the popover closed is the click
/// that closed it.
const DISMISS_GRACE: Duration = Duration::from_millis(400);

thread_local! {
    static DISMISSED: RefCell<Option<(glib::WeakRef<gtk::Widget>, Instant)>> =
        const { RefCell::new(None) };
}

/// Whether `w`'s popover was dismissed a moment ago: the press that
/// dismissed it reaches the anchor too on some setups, and must not open it
/// again.
pub fn just_dismissed_from(w: &impl IsA<gtk::Widget>) -> bool {
    DISMISSED.with(|d| {
        d.borrow().as_ref().is_some_and(|(anchor, at)| {
            at.elapsed() < DISMISS_GRACE && anchor.upgrade().as_ref() == Some(w.upcast_ref())
        })
    })
}

struct PickerW {
    id: u64,
    anchor: gtk::Widget,
    /// Where the anchor was when the popover was last placed, in the
    /// window's coordinates.
    placed_at: Rc<Cell<Option<(f32, f32)>>>,
    popover: gtk::Popover,
    entry: gtk::Entry,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    items: RefCell<Vec<ListItem>>,
    selected: Rc<Cell<Option<usize>>>,
    on_query: Slot<String>,
    on_move: Slot<i32>,
    on_choose: Slot<usize>,
    on_dismiss: Slot<()>,
}

#[derive(Default)]
pub struct PickerHost {
    current: RefCell<Option<PickerW>>,
}

impl PickerHost {
    pub fn render(&self, want: Option<&Popover>, list: &ListW) {
        let anchor = want.and_then(|p| list.element(&p.anchor.0, p.anchor.1));
        let keep = {
            let cur = self.current.borrow();
            match (cur.as_ref(), want, &anchor) {
                (Some(c), Some(p), Some(a)) => c.id == p.id && &c.anchor == a,
                _ => false,
            }
        };
        if !keep {
            self.close();
        }
        let (Some(p), Some(anchor)) = (want, anchor) else {
            return;
        };
        if self.current.borrow().is_none() {
            let w = PickerW::open(p, anchor);
            w.popup_when_shown(list);
            *self.current.borrow_mut() = Some(w);
        }
        if let Some(c) = self.current.borrow().as_ref() {
            c.patch(&p.list);
            c.follow_anchor();
        }
    }

    /// Close without reporting: the view no longer has it.
    pub fn close(&self) {
        if let Some(c) = self.current.borrow_mut().take() {
            quietly(|| c.popover.popdown());
            c.popover.unparent();
        }
    }

    /// The filter field (tests).
    pub fn entry(&self) -> Option<gtk::Entry> {
        self.current.borrow().as_ref().map(|c| c.entry.clone())
    }

    pub fn popover(&self) -> Option<gtk::Popover> {
        self.current.borrow().as_ref().map(|c| c.popover.clone())
    }
}

impl PickerW {
    fn open(p: &Popover, anchor: gtk::Widget) -> PickerW {
        let popover = gtk::Popover::new();
        popover.add_css_class("wtm-picker");
        popover.set_position(gtk::PositionType::Bottom);
        let entry = gtk::Entry::new();
        entry.set_hexpand(true);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&list));
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(320);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_size_request(300, -1);
        column.append(&entry);
        column.append(&scroll);
        popover.set_child(Some(&column));
        popover.set_parent(&anchor);

        let w = PickerW {
            id: p.id,
            anchor: anchor.clone(),
            placed_at: Rc::new(Cell::new(None)),
            popover: popover.clone(),
            entry: entry.clone(),
            list: list.clone(),
            scroll,
            items: RefCell::new(Vec::new()),
            selected: Rc::new(Cell::new(None)),
            on_query: slot(&p.list.on_query),
            on_move: slot(&p.list.on_move),
            on_choose: slot(&p.list.on_choose),
            on_dismiss: slot(&p.list.on_dismiss),
        };

        let s = w.on_query.clone();
        entry.connect_changed(move |e| fire(&s, e.text().to_string()));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (mv, choose, sel) = (w.on_move.clone(), w.on_choose.clone(), w.selected.clone());
        keys.connect_key_pressed(move |_, key, _, _| match key {
            gdk::Key::Up | gdk::Key::KP_Up => {
                fire(&mv, -1);
                glib::Propagation::Stop
            }
            gdk::Key::Down | gdk::Key::KP_Down => {
                fire(&mv, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                if let Some(i) = sel.get() {
                    fire(&choose, i);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        entry.add_controller(keys);
        let choose = w.on_choose.clone();
        list.connect_row_activated(move |_, row| {
            if let Ok(i) = usize::try_from(row.index()) {
                fire(&choose, i);
            }
        });
        let (dismiss, a) = (w.on_dismiss.clone(), anchor.downgrade());
        popover.connect_closed(move |_| {
            if rendering() {
                return;
            }
            DISMISSED.with(|d| *d.borrow_mut() = Some((a.clone(), Instant::now())));
            fire(&dismiss, ());
        });

        w.patch(&p.list);
        w
    }

    /// Pop up once the anchor is on screen. The anchor is the selected
    /// row's, which the keyboard can leave scrolled out of view (Ctrl+T),
    /// and a popover hung from somewhere outside the list's visible part
    /// is placed off screen, invisible, with the keyboard captured. So the
    /// row is scrolled into view first, and the popover waits for the
    /// layout that brings it there. An anchor that never shows (a closed
    /// card) is given up after about a second, as if the user had
    /// dismissed it.
    fn popup_when_shown(&self, list: &ListW) {
        if list.shows(&self.anchor) {
            popup(&self.popover, &self.entry, &self.anchor, &self.placed_at);
            return;
        }
        let (anchor, popover, entry) = (
            self.anchor.downgrade(),
            self.popover.downgrade(),
            self.entry.downgrade(),
        );
        let (weak_list, dismiss, placed_at) =
            (list.weak(), self.on_dismiss.clone(), self.placed_at.clone());
        let frames = Cell::new(0u32);
        list.scroller.add_tick_callback(move |_, _| {
            let (Some(list), Some(anchor), Some(popover), Some(entry)) = (
                weak_list.upgrade(),
                anchor.upgrade(),
                popover.upgrade(),
                entry.upgrade(),
            ) else {
                return glib::ControlFlow::Break;
            };
            // Closed by the view meanwhile.
            if popover.parent().as_ref() != Some(&anchor) {
                return glib::ControlFlow::Break;
            }
            if list.shows(&anchor) {
                popup(&popover, &entry, &anchor, &placed_at);
                return glib::ControlFlow::Break;
            }
            // Outside the render, so the scroll is reported as the user's:
            // their Ctrl+T asked for it.
            if frames.get() == 0 {
                list.reveal_widget(&anchor, 12.0);
            }
            frames.set(frames.get() + 1);
            if frames.get() > 60 {
                fire(&dismiss, ());
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    /// Place the popover again when a render moved its anchor (a row added
    /// above it): GTK places a popover when it pops up, and a plain button
    /// does not move the popovers hung from it. Checked after the render's
    /// layout, when the anchor is where it will be.
    fn follow_anchor(&self) {
        if !self.popover.is_visible() {
            return;
        }
        let (anchor, popover, placed_at) = (
            self.anchor.downgrade(),
            self.popover.downgrade(),
            self.placed_at.clone(),
        );
        self.anchor.add_tick_callback(move |_, _| {
            if let (Some(anchor), Some(popover)) = (anchor.upgrade(), popover.upgrade()) {
                let at = anchor_position(&anchor);
                if popover.is_visible() && at.is_some() && at != placed_at.get() {
                    placed_at.set(at);
                    popover.present();
                }
            }
            glib::ControlFlow::Break
        });
    }

    fn patch(&self, f: &FilterList) {
        put(&self.on_query, &f.on_query);
        put(&self.on_move, &f.on_move);
        put(&self.on_choose, &f.on_choose);
        put(&self.on_dismiss, &f.on_dismiss);
        crate::elements::set_entry(&self.entry, &f.query, &f.placeholder, true);
        if *self.items.borrow() != f.items {
            while let Some(row) = self.list.row_at_index(0) {
                self.list.remove(&row);
            }
            for item in &f.items {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let check = gtk::Image::from_icon_name("object-select-symbolic");
                check.set_pixel_size(12);
                // Kept in place when unchecked, so every name lines up.
                check.set_opacity(if item.checked { 1.0 } else { 0.0 });
                line.append(&check);
                let label = RichLabel::new(
                    &item.label,
                    LabelOpts {
                        ellipsize: true,
                        ..LabelOpts::default()
                    },
                );
                label.root.add_css_class("t-branch");
                line.append(&label.root);
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&line));
                // The field keeps the keyboard; rows are for the pointer.
                row.set_focusable(false);
                self.list.append(&row);
            }
            *self.items.borrow_mut() = f.items.clone();
        }
        self.selected.set(f.selected);
        let row = f.selected.and_then(|i| self.list.row_at_index(i as i32));
        if self.list.selected_row() != row {
            self.list.select_row(row.as_ref());
        }
        if let Some(row) = row {
            let scroll = self.scroll.clone();
            self.list.add_tick_callback(move |_, _| {
                if let Some(b) = row.compute_bounds(&scroll) {
                    let adj = scroll.vadjustment();
                    let (top, bottom) = (b.y() as f64, (b.y() + b.height()) as f64);
                    if top < 0.0 {
                        adj.set_value(adj.value() + top);
                    } else if bottom > adj.page_size() {
                        adj.set_value(adj.value() + bottom - adj.page_size());
                    }
                }
                glib::ControlFlow::Break
            });
        }
    }
}

/// Pop up with the keyboard in the filter field, noting where the anchor is.
fn popup(
    popover: &gtk::Popover,
    entry: &gtk::Entry,
    anchor: &gtk::Widget,
    placed_at: &Cell<Option<(f32, f32)>>,
) {
    placed_at.set(anchor_position(anchor));
    popover.popup();
    entry.grab_focus();
}

/// Where `anchor` is in its window.
fn anchor_position(anchor: &gtk::Widget) -> Option<(f32, f32)> {
    let root = anchor.root()?;
    let b = anchor.compute_bounds(&root)?;
    Some((b.x(), b.y()))
}
