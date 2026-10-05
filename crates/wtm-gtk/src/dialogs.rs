//! Modal questions: the first dialog of the view, shown as a modal window on
//! the main one. A button closes it and then runs its handler; a dialog that
//! leaves the view is closed without running anything; a dialog this backend
//! closed is never shown again, even while the view still lists it (its
//! handler's message has not been handled yet).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, glib};

use wtm_toolkit::{Axis, Dialog, DialogButton, DialogStyle, Role};

use crate::elements::Node;
use crate::style;
use crate::util::{put, rendering, set_label_text, show, slot, Slot};

#[derive(Default)]
struct State {
    current: RefCell<Option<DialogW>>,
    closed: RefCell<HashSet<u64>>,
}

pub struct DialogHost {
    state: Rc<State>,
}

struct ButtonW {
    button: gtk::Button,
    on_press: Slot<()>,
    last: DialogButton,
}

struct DialogW {
    id: u64,
    window: gtk::Window,
    icon: gtk::Image,
    title: gtk::Label,
    message: gtk::Label,
    body_box: gtk::Box,
    body: RefCell<Option<Node>>,
    buttons_box: gtk::Box,
    buttons: RefCell<Vec<ButtonW>>,
}

impl Default for DialogHost {
    fn default() -> Self {
        DialogHost {
            state: Rc::new(State::default()),
        }
    }
}

impl DialogHost {
    pub fn render(&self, dialogs: &[Dialog], parent: &gtk::Window) {
        let want = dialogs
            .first()
            .filter(|d| !self.state.closed.borrow().contains(&d.id));
        let same = matches!(
            (self.state.current.borrow().as_ref(), want),
            (Some(c), Some(d)) if c.id == d.id
        );
        if !same {
            if let Some(gone) = self.state.current.borrow_mut().take() {
                gone.window.destroy();
            }
        }
        let Some(d) = want else { return };
        if self.state.current.borrow().is_none() {
            let w = DialogW::new(d, parent, Rc::downgrade(&self.state));
            *self.state.current.borrow_mut() = Some(w);
            let cur = self.state.current.borrow();
            let w = cur.as_ref().expect("just set");
            w.window.present();
            w.focus_first(d);
        } else if let Some(w) = self.state.current.borrow().as_ref() {
            w.patch(d, &Rc::downgrade(&self.state));
        }
    }

    /// The open dialog's window (screenshots and tests).
    pub fn window(&self) -> Option<gtk::Window> {
        self.state
            .current
            .borrow()
            .as_ref()
            .map(|d| d.window.clone())
    }

    /// A field or button of the open dialog's body, by id (tests).
    pub fn element(&self, id: &str) -> Option<gtk::Widget> {
        let cur = self.state.current.borrow();
        let d = cur.as_ref()?;
        let body = d.body.borrow();
        body.as_ref()?.find(id)
    }
}

/// Press button `i` of the open dialog: close it, then run its handler.
fn press(state: &Weak<State>, i: usize) {
    if rendering() {
        return;
    }
    let Some(state) = state.upgrade() else { return };
    let enabled = state
        .current
        .borrow()
        .as_ref()
        .and_then(|d| d.buttons.borrow().get(i).map(|b| b.last.enabled))
        .unwrap_or(false);
    if !enabled {
        return;
    }
    let Some(d) = state.current.borrow_mut().take() else {
        return;
    };
    state.closed.borrow_mut().insert(d.id);
    let handler = d.buttons.borrow()[i].on_press.borrow().clone();
    let parent = d.window.transient_for();
    d.window.destroy();
    if let Some(p) = parent {
        p.present();
    }
    handler.call(());
}

fn role_index(state: &Weak<State>, role: Role) -> Option<usize> {
    let state = state.upgrade()?;
    let cur = state.current.borrow();
    let d = cur.as_ref()?;
    let buttons = d.buttons.borrow();
    buttons.iter().position(|b| b.last.role == role)
}

fn button_count(state: &Weak<State>) -> usize {
    state
        .upgrade()
        .and_then(|s| {
            s.current
                .borrow()
                .as_ref()
                .map(|d| d.buttons.borrow().len())
        })
        .unwrap_or(0)
}

/// What dismissing the dialog presses: Cancel, or the only button of a
/// dialog that just says something. A dialog with neither stays open: which
/// of its answers dismissing means is not this backend's to guess.
fn dismiss_index(state: &Weak<State>) -> Option<usize> {
    role_index(state, Role::Cancel).or_else(|| (button_count(state) == 1).then_some(0))
}

impl DialogW {
    fn new(d: &Dialog, parent: &gtk::Window, state: Weak<State>) -> DialogW {
        let window = gtk::Window::new();
        window.set_modal(true);
        window.set_transient_for(Some(parent));
        window.set_resizable(false);
        window.set_destroy_with_parent(true);
        window.set_title(Some(&d.title));
        // No title bar: the title is the dialog's heading, as GNOME's own
        // alerts have it. The whole dialog is its handle instead.
        let no_titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        no_titlebar.set_visible(false);
        window.set_titlebar(Some(&no_titlebar));
        style::adopt(&window);

        let icon = gtk::Image::new();
        icon.set_pixel_size(40);
        icon.set_valign(gtk::Align::Start);
        icon.add_css_class("wtm-dialog-icon");
        let title = crate::util::label("", &["wtm-dialog-title"]);
        title.set_wrap(true);
        title.set_max_width_chars(48);
        let message = crate::util::label("", &[]);
        message.set_wrap(true);
        message.set_max_width_chars(52);
        message.set_width_chars(36);
        message.set_selectable(false);
        let body_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body_box.set_margin_top(6);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 6);
        text.append(&title);
        text.append(&message);
        text.append(&body_box);
        text.set_hexpand(true);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        top.append(&icon);
        top.append(&text);
        let buttons_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons_box.set_halign(gtk::Align::End);
        buttons_box.set_margin_top(14);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.add_css_class("wtm-dialog");
        column.append(&top);
        column.append(&buttons_box);
        let handle = gtk::WindowHandle::new();
        handle.set_child(Some(&column));
        window.set_child(Some(&handle));

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (s, w) = (state.clone(), window.downgrade());
        keys.connect_key_pressed(move |_, key, _, mods| {
            if mods.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Escape => {
                    if let Some(i) = dismiss_index(&s) {
                        press(&s, i);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                    // A button with the keyboard is pressed by Return like
                    // Space, as everywhere: a Return that ran Delete while
                    // Cancel was focused would be a trap.
                    let on_button = w
                        .upgrade()
                        .and_then(|w| gtk::prelude::RootExt::focus(&w))
                        .is_some_and(|f| f.is::<gtk::Button>());
                    if on_button {
                        return glib::Propagation::Proceed;
                    }
                    press(&s, 0);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        window.add_controller(keys);
        let s = state.clone();
        // The window manager's close (Alt+F4) is the same as Escape.
        window.connect_close_request(move |_| {
            if let Some(i) = dismiss_index(&s) {
                press(&s, i);
            }
            glib::Propagation::Stop
        });

        let w = DialogW {
            id: d.id,
            window,
            icon,
            title,
            message,
            body_box,
            body: RefCell::new(None),
            buttons_box,
            buttons: RefCell::new(Vec::new()),
        };
        w.patch(d, &state);
        w
    }

    fn patch(&self, d: &Dialog, state: &Weak<State>) {
        set_label_text(&self.title, &d.title);
        set_label_text(&self.message, &d.message);
        show(&self.message, !d.message.is_empty());
        let (icon, class) = match d.style {
            DialogStyle::Info => (None, None),
            DialogStyle::Warning => (Some("dialog-warning-symbolic"), Some("warning")),
            DialogStyle::Critical => (Some("dialog-warning-symbolic"), Some("critical")),
        };
        show(&self.icon, icon.is_some());
        if let Some(name) = icon {
            self.icon.set_icon_name(Some(name));
        }
        style::swap_classes(&self.icon, &["warning", "critical"], &[]);
        if let Some(c) = class {
            self.icon.add_css_class(c);
        }

        let mut body = self.body.borrow_mut();
        let patched = match (body.as_mut(), &d.body) {
            (Some(node), Some(el)) => node.patch(el, Axis::Vertical),
            _ => false,
        };
        if !patched {
            let el = &d.body;
            {
                if let Some(old) = body.take().and_then(|n| n.widget) {
                    self.body_box.remove(&old);
                }
                if let Some(el) = el {
                    let node = Node::build(el, Axis::Vertical);
                    if let Some(w) = &node.widget {
                        self.body_box.append(w);
                    }
                    *body = Some(node);
                }
            }
        }
        show(&self.body_box, body.is_some());
        drop(body);

        let same_buttons = {
            let b = self.buttons.borrow();
            b.len() == d.buttons.len()
                && b.iter()
                    .zip(&d.buttons)
                    .all(|(w, v)| w.last.label == v.label && w.last.role == v.role)
        };
        if !same_buttons {
            while let Some(c) = self.buttons_box.first_child() {
                self.buttons_box.remove(&c);
            }
            let mut out = Vec::new();
            // In order of importance, the default last: GNOME puts the
            // affirmative button at the trailing end.
            for (i, b) in d.buttons.iter().enumerate() {
                let button = gtk::Button::with_label(&b.label);
                match b.role {
                    Role::Default => button.add_css_class("suggested-action"),
                    Role::Destructive => button.add_css_class("destructive-action"),
                    Role::Cancel | Role::Normal => {}
                }
                let s = state.clone();
                button.connect_clicked(move |_| press(&s, i));
                self.buttons_box.prepend(&button);
                out.push(ButtonW {
                    button,
                    on_press: slot(&b.on_press),
                    last: b.clone(),
                });
            }
            *self.buttons.borrow_mut() = out;
        }
        for (w, b) in self.buttons.borrow_mut().iter_mut().zip(&d.buttons) {
            put(&w.on_press, &b.on_press);
            crate::util::set_sensitive(&w.button, b.enabled);
            w.last = b.clone();
        }
        if let Some(first) = self.buttons.borrow().first() {
            self.window.set_default_widget(Some(&first.button));
        }
    }

    /// The keyboard goes to the named field; with none, to Cancel when the
    /// default would destroy something, else to the default.
    fn focus_first(&self, d: &Dialog) {
        if let Some(id) = d.focus {
            if let Some(w) = self.body.borrow().as_ref().and_then(|b| b.find(id)) {
                crate::util::focus_quietly(&w);
                return;
            }
        }
        let buttons = self.buttons.borrow();
        let destructive = d
            .buttons
            .first()
            .is_some_and(|b| b.role == Role::Destructive);
        let target = if destructive {
            d.buttons.iter().position(|b| b.role == Role::Cancel)
        } else {
            Some(0)
        };
        if let Some(b) = target.and_then(|i| buttons.get(i)) {
            b.button.grab_focus();
        }
    }
}
