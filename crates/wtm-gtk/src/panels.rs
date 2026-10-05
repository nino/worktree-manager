//! Secondary windows (Settings), keyed by the view. Closing one is reported;
//! one the view drops is closed without a word.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use wtm_toolkit::{Axis, Key, Panel};

use crate::elements::Node;
use crate::style;
use crate::util::{fire, put, slot, Slot};

struct PanelW {
    window: gtk::Window,
    body_box: gtk::Box,
    body: RefCell<Node>,
    on_close: Slot<()>,
}

#[derive(Default)]
pub struct PanelHost {
    open: Rc<RefCell<HashMap<Key, PanelW>>>,
}

impl PanelHost {
    pub fn render(&self, panels: &[Panel], parent: &gtk::Window, app: Option<&gtk::Application>) {
        let gone: Vec<Key> = self
            .open
            .borrow()
            .keys()
            .filter(|k| !panels.iter().any(|p| &p.key == *k))
            .cloned()
            .collect();
        for k in gone {
            if let Some(p) = self.open.borrow_mut().remove(&k) {
                p.window.destroy();
            }
        }
        for p in panels {
            let exists = self.open.borrow().contains_key(&p.key);
            if exists {
                let open = self.open.borrow();
                let w = &open[&p.key];
                put(&w.on_close, &p.on_close);
                if w.window.title().as_deref() != Some(p.title.as_str()) {
                    w.window.set_title(Some(&p.title));
                }
                let mut body = w.body.borrow_mut();
                if !body.patch(&p.body, Axis::Vertical) {
                    if let Some(old) = &body.widget {
                        w.body_box.remove(old);
                    }
                    *body = Node::build(&p.body, Axis::Vertical);
                    if let Some(new) = &body.widget {
                        w.body_box.append(new);
                    }
                }
            } else {
                let w = self.make(p, parent, app);
                w.window.present();
                if let Some(f) = p.focus.and_then(|id| w.body.borrow().find(id)) {
                    crate::util::focus_quietly(&f);
                }
                self.open.borrow_mut().insert(p.key.clone(), w);
            }
        }
    }

    fn make(&self, p: &Panel, parent: &gtk::Window, app: Option<&gtk::Application>) -> PanelW {
        let window = gtk::Window::new();
        window.set_title(Some(&p.title));
        window.set_transient_for(Some(parent));
        window.set_destroy_with_parent(true);
        window.set_resizable(false);
        // A header bar of its own, so the title and close button are drawn
        // the same with or without a window manager that decorates.
        window.set_titlebar(Some(&gtk::HeaderBar::new()));
        if let Some(app) = app {
            window.set_application(Some(app));
        }
        style::adopt(&window);
        let body = Node::build(&p.body, Axis::Vertical);
        let body_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body_box.add_css_class("wtm-panel");
        body_box.set_size_request(540, -1);
        if let Some(w) = &body.widget {
            body_box.append(w);
        }
        window.set_child(Some(&body_box));
        let on_close = slot(&p.on_close);
        let (open, key, s) = (self.open.clone(), p.key.clone(), on_close.clone());
        window.connect_close_request(move |_| {
            // Forgotten here before the view drops it, so the next render
            // does not destroy a window that is already going.
            open.borrow_mut().remove(&key);
            fire(&s, ());
            glib::Propagation::Proceed
        });
        let keys = gtk::EventControllerKey::new();
        let w = window.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(w) = w.upgrade() {
                    w.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        PanelW {
            window,
            body_box,
            body: RefCell::new(body),
            on_close,
        }
    }

    pub fn present(&self, key: &str) {
        if let Some(p) = self.open.borrow().get(key) {
            p.window.present();
        }
    }

    /// Some open panel's window (screenshots).
    pub fn any_window(&self) -> Option<gtk::Window> {
        self.open.borrow().values().next().map(|p| p.window.clone())
    }

    pub fn window(&self, key: &str) -> Option<gtk::Window> {
        self.open.borrow().get(key).map(|p| p.window.clone())
    }
}
