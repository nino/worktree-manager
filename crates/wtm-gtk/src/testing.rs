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

/// Follow the desktop's light or dark preference, as the app does.
pub fn follow_desktop_appearance() {
    crate::style::init_appearance();
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
    /// A backend with an application: it has the menus.
    pub fn with_app(app: &gtk::Application) -> Probe {
        Probe {
            backend: GtkBackend::new(Some(app.clone())),
        }
    }

    /// The header bar's menu button, when there is an application.
    pub fn menu_button(&self) -> Option<gtk::MenuButton> {
        let menus = self.backend.inner.menus.borrow();
        menus.as_ref().map(|m| m.button.clone())
    }

    pub fn render(&self, view: &View) {
        self.backend.render(view);
        pump();
    }

    /// Render and carry out `effects` with no main loop in between, as one
    /// batch of the runtime does: nothing is laid out yet when they run.
    pub fn render_then(&self, view: &View, effects: Vec<Effect>) {
        self.backend.render(view);
        for e in effects {
            self.backend.perform(e);
        }
        pump();
    }

    fn list_adjustment(&self) -> Option<gtk::Adjustment> {
        let main = self.backend.inner.main.borrow();
        Some(main.as_ref()?.list.scroller.vadjustment())
    }

    /// The list's scroll offset.
    pub fn scroll_value(&self) -> f64 {
        self.list_adjustment().map_or(0.0, |a| a.value())
    }

    /// The furthest the list can scroll.
    pub fn scroll_max(&self) -> f64 {
        self.list_adjustment()
            .map_or(0.0, |a| (a.upper() - a.page_size()).max(0.0))
    }

    /// The offset last reported to the program as the user's scroll.
    pub fn reported_scroll(&self) -> Option<f64> {
        let main = self.backend.inner.main.borrow();
        main.as_ref()?.list.reported_scroll()
    }

    /// Whether `w` is laid out and wholly in the list's visible part.
    pub fn shows(&self, w: &gtk::Widget) -> bool {
        let main = self.backend.inner.main.borrow();
        main.as_ref().is_some_and(|m| m.list.shows(w))
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
