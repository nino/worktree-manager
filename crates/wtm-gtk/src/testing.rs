//! A backend without an application, for the conformance tests: render a
//! view, look at the widgets, poke them as a user would. Not part of the
//! app's API.

use gtk::glib;
use gtk::prelude::*;

use wtm_toolkit::{Backend, Effect, View};

use crate::backend::GtkBackend;

/// `gtk::init`, with the stylesheet. `false` when there is no display.
pub fn init() -> bool {
    if gtk::init().is_err() {
        return false;
    }
    crate::style::install();
    true
}

/// Run the main loop until nothing is pending.
pub fn pump() {
    let ctx = glib::MainContext::default();
    while ctx.iteration(false) {}
}

pub struct Probe {
    backend: GtkBackend,
}

impl Default for Probe {
    fn default() -> Self {
        Probe {
            backend: GtkBackend::new(None),
        }
    }
}

impl Probe {
    pub fn render(&self, view: &View) {
        self.backend.render(view);
        pump();
    }

    pub fn perform(&self, effect: Effect) {
        self.backend.perform(effect);
        pump();
    }

    pub fn window(&self) -> Option<gtk::Window> {
        self.backend.inner.window.borrow().clone()
    }

    /// The row widget for a section's or a row's key.
    pub fn row(&self, key: &str) -> Option<gtk::Widget> {
        let main = self.backend.inner.main.borrow();
        main.as_ref()?.list.widget(key)
    }

    /// An element of a row by id (`branch`, `push`, `copy-path`…).
    pub fn element(&self, key: &str, id: &str) -> Option<gtk::Widget> {
        let main = self.backend.inner.main.borrow();
        main.as_ref()?.list.element(key, id)
    }

    pub fn badges(&self, key: &str) -> Vec<String> {
        let main = self.backend.inner.main.borrow();
        main.as_ref()
            .map(|m| m.list.badges(key))
            .unwrap_or_default()
    }

    pub fn is_selected(&self, key: &str) -> bool {
        self.row(key).is_some_and(|w| w.has_css_class("selected"))
    }

    pub fn dialog(&self) -> Option<gtk::Window> {
        self.backend.inner.dialogs.window()
    }

    pub fn dialog_element(&self, id: &str) -> Option<gtk::Widget> {
        self.backend.inner.dialogs.element(id)
    }

    pub fn picker_entry(&self) -> Option<gtk::Entry> {
        self.backend.inner.picker.entry()
    }

    pub fn search(&self) -> Option<gtk::SearchEntry> {
        let main = self.backend.inner.main.borrow();
        main.as_ref()?.search.clone()
    }

    /// The widget with the keyboard in the main window or the dialog.
    pub fn focus(&self) -> Option<gtk::Widget> {
        let d = self.dialog().and_then(|d| gtk::prelude::RootExt::focus(&d));
        d.or_else(|| self.window().and_then(|w| gtk::prelude::RootExt::focus(&w)))
    }
}
