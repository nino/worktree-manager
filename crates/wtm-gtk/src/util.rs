//! Small pieces every part of the backend uses: handler slots, the
//! "rendering now" flag that keeps self-made changes from being reported,
//! and the icon button.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use wtm_toolkit::{Handler, Icon};

use crate::draw;

// MARK: Handlers

/// Where a widget finds its current handler. Signals are connected once,
/// when the widget is made, and read the slot when they fire; every render
/// puts the view's new handler in it (a handler captures what its row showed
/// when it was built, so an old one would act on that).
pub type Slot<A> = Rc<RefCell<Handler<A>>>;

pub fn slot<A>(h: &Handler<A>) -> Slot<A> {
    Rc::new(RefCell::new(h.clone()))
}

pub fn put<A>(s: &Slot<A>, h: &Handler<A>) {
    *s.borrow_mut() = h.clone();
}

/// Call the slot's handler for something the user did. Ignored while the
/// backend is rendering: a signal fired then (a field's text set, a section
/// opened, focus moved off a row being removed) is the backend's own change.
pub fn fire<A>(s: &Slot<A>, arg: A) {
    if rendering() {
        return;
    }
    let h = s.borrow().clone();
    h.call(arg);
}

thread_local! {
    static RENDERING: Cell<u32> = const { Cell::new(0) };
}

pub fn rendering() -> bool {
    RENDERING.with(|r| r.get() > 0)
}

/// Run `f` as part of a render: no signal it causes is reported.
pub fn quietly<R>(f: impl FnOnce() -> R) -> R {
    RENDERING.with(|r| r.set(r.get() + 1));
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            RENDERING.with(|r| r.set(r.get() - 1));
        }
    }
    let _done = Done;
    f()
}

// MARK: Focus

/// Whether the keyboard is in `w` or something inside it (an entry's focus
/// is on its inner text widget).
pub fn has_keyboard(w: &impl IsA<gtk::Widget>) -> bool {
    let w = w.upcast_ref::<gtk::Widget>();
    let Some(focus) = w.root().and_then(|r| r.focus()) else {
        return false;
    };
    &focus == w || focus.is_ancestor(w)
}

/// Give `w` the keyboard as the view asked. A field is not selected the way
/// Tab would select it: a window that opens with a path highlighted invites
/// typing over it. The caret goes to the end, where a path's leaf is.
pub fn focus_quietly(w: &gtk::Widget) {
    if let Some(e) = w.downcast_ref::<gtk::Entry>() {
        e.grab_focus_without_selecting();
        e.set_position(-1);
    } else {
        w.grab_focus();
    }
}

// MARK: Buttons

/// Tooltip, and what assistive technology reads: the name (for a button
/// with only an icon) and the help.
pub fn set_hint(w: &impl IsA<gtk::Widget>, hint: &str, is_name: bool) {
    if w.tooltip_text().as_deref() != Some(hint) {
        w.set_tooltip_text(Some(hint));
    }
    let mut props = vec![gtk::accessible::Property::Description(hint)];
    if is_name {
        props.push(gtk::accessible::Property::Label(hint));
    }
    w.upcast_ref::<gtk::Widget>().update_property(&props);
}

/// A borderless glyph button. Takes the keyboard on Tab like every button.
pub fn icon_button(icon: Icon, hint: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&draw::icon(icon, 14)));
    b.add_css_class("flat");
    b.add_css_class("wtm-icon");
    b.set_valign(gtk::Align::Center);
    set_hint(&b, hint, true);
    b
}

/// Connect a button's click to a slot.
pub fn on_click(b: &gtk::Button, s: &Slot<()>) {
    let s = s.clone();
    b.connect_clicked(move |_| fire(&s, ()));
}

/// Set a widget's visibility only when it changes.
pub fn show(w: &impl IsA<gtk::Widget>, visible: bool) {
    if w.is_visible() != visible {
        w.set_visible(visible);
    }
}

pub fn set_sensitive(w: &impl IsA<gtk::Widget>, on: bool) {
    if w.is_sensitive() != on {
        w.set_sensitive(on);
    }
}

pub fn set_label_text(l: &gtk::Label, text: &str) {
    if l.text() != text {
        l.set_text(text);
    }
}

pub fn set_tooltip(w: &impl IsA<gtk::Widget>, tip: Option<&str>) {
    if w.tooltip_text().as_deref() != tip {
        w.set_tooltip_text(tip);
    }
}

pub fn spin(s: &gtk::Spinner, on: bool) {
    if s.is_spinning() != on {
        s.set_spinning(on);
    }
}

/// A plain label in one of the text styles.
pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}
