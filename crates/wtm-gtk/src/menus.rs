//! The menus. GNOME has no menu bar, so every menu's items go in the header
//! bar's primary menu, one section per group, the application menu's last
//! (Settings, About, Quit), and the shortcuts become application
//! accelerators with Ctrl for the primary modifier.
//!
//! A shortcut is bound to an action of its own that is always enabled: it
//! drains the queue first (`wtm_toolkit::flush`) and then checks the item,
//! so Ctrl+N straight after an arrow key sees the new selection rather than
//! the state the menu was last rendered with.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use wtm_toolkit::{KeyName, Menu, MenuItem, MenuRole, Shortcut, Standard};

use crate::util::{fire, put, slot, Slot};

struct ItemState {
    on_select: Slot<()>,
    enabled: Cell<bool>,
    action: gio::SimpleAction,
}

/// What a rebuild depends on: everything but enabled state and handlers.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Action(String, Option<Shortcut>),
    Separator,
    Standard(Standard),
}

pub struct MenuHost {
    app: gtk::Application,
    pub button: gtk::MenuButton,
    model: gio::Menu,
    items: Rc<RefCell<HashMap<String, ItemState>>>,
    installed: RefCell<Vec<String>>,
    shape: RefCell<Vec<(MenuRole, Vec<Shape>)>>,
    quit: Rc<dyn Fn()>,
}

/// The accelerator for a shortcut: Ctrl, plus Shift and Alt as asked.
pub fn accel(s: &Shortcut) -> String {
    let mut out = String::from("<Control>");
    if s.shift {
        out.push_str("<Shift>");
    }
    if s.alt {
        out.push_str("<Alt>");
    }
    match s.key {
        KeyName::Char(c) => out.push_str(&key_name(c)),
        KeyName::Up => out.push_str("Up"),
        KeyName::Down => out.push_str("Down"),
    }
    out
}

/// GDK's name for the key that types `c`.
fn key_name(c: char) -> String {
    let name = match c {
        ',' => "comma",
        '.' => "period",
        '/' => "slash",
        ';' => "semicolon",
        '\'' => "apostrophe",
        '[' => "bracketleft",
        ']' => "bracketright",
        '-' => "minus",
        '=' => "equal",
        '`' => "grave",
        '\\' => "backslash",
        ' ' => "space",
        c => return c.to_ascii_lowercase().to_string(),
    };
    name.into()
}

/// An action name for a label: `New Worktree…` → `new-worktree`.
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The roles in the order their sections appear in the primary menu.
const ORDER: [MenuRole; 4] = [
    MenuRole::File,
    MenuRole::Edit,
    MenuRole::Window,
    MenuRole::App,
];

/// How a standard item is labelled here, its accelerator for display, and
/// whether GTK has it at all.
fn standard(s: Standard) -> Option<(&'static str, Option<&'static str>)> {
    Some(match s {
        Standard::About => ("About Worktree Manager", None),
        Standard::Quit => ("Quit", Some("<Control>q")),
        Standard::CloseWindow => ("Close Window", Some("<Control>w")),
        Standard::Undo => ("Undo", Some("<Control>z")),
        Standard::Redo => ("Redo", Some("<Control><Shift>z")),
        Standard::Cut => ("Cut", Some("<Control>x")),
        Standard::Copy => ("Copy", Some("<Control>c")),
        Standard::Paste => ("Paste", Some("<Control>v")),
        Standard::SelectAll => ("Select All", Some("<Control>a")),
        Standard::Minimize => ("Minimize", None),
        Standard::Zoom => ("Maximize", None),
        Standard::Services
        | Standard::Hide
        | Standard::HideOthers
        | Standard::ShowAll
        | Standard::BringAllToFront => return None,
    })
}

impl MenuHost {
    pub fn new(app: &gtk::Application, quit: Rc<dyn Fn()>) -> MenuHost {
        let model = gio::Menu::new();
        let button = gtk::MenuButton::new();
        button.set_icon_name("open-menu-symbolic");
        button.set_menu_model(Some(&model));
        button.set_primary(true);
        crate::util::set_hint(&button, "Main menu", true);
        MenuHost {
            app: app.clone(),
            button,
            model,
            items: Rc::default(),
            installed: RefCell::new(Vec::new()),
            shape: RefCell::new(Vec::new()),
            quit,
        }
    }

    pub fn render(&self, menus: &[Menu]) {
        let shape: Vec<(MenuRole, Vec<Shape>)> = menus
            .iter()
            .map(|m| {
                (
                    m.role,
                    m.items
                        .iter()
                        .map(|i| match i {
                            MenuItem::Action {
                                label, shortcut, ..
                            } => Shape::Action(label.clone(), *shortcut),
                            MenuItem::Separator => Shape::Separator,
                            MenuItem::Standard(s) => Shape::Standard(*s),
                        })
                        .collect(),
                )
            })
            .collect();
        if *self.shape.borrow() != shape {
            self.rebuild(menus);
            *self.shape.borrow_mut() = shape;
        }
        let items = self.items.borrow();
        for m in menus {
            for i in &m.items {
                if let MenuItem::Action {
                    label,
                    enabled,
                    on_select,
                    ..
                } = i
                {
                    if let Some(st) = items.get(&slug(label)) {
                        put(&st.on_select, on_select);
                        st.enabled.set(*enabled);
                        if st.action.is_enabled() != *enabled {
                            st.action.set_enabled(*enabled);
                        }
                    }
                }
            }
        }
    }

    fn rebuild(&self, menus: &[Menu]) {
        for name in self.installed.borrow_mut().drain(..) {
            self.app.remove_action(&name);
            self.app.set_accels_for_action(&format!("app.{name}"), &[]);
        }
        self.items.borrow_mut().clear();
        self.model.remove_all();
        for role in ORDER {
            for m in menus.iter().filter(|m| m.role == role) {
                let mut section = gio::Menu::new();
                for item in &m.items {
                    match item {
                        MenuItem::Separator => {
                            if section.n_items() > 0 {
                                self.model.append_section(None, &section);
                                section = gio::Menu::new();
                            }
                        }
                        MenuItem::Action {
                            label, shortcut, ..
                        } => self.add_action(&section, label, shortcut.as_ref()),
                        MenuItem::Standard(s) => self.add_standard(&section, *s),
                    }
                }
                if section.n_items() > 0 {
                    self.model.append_section(None, &section);
                }
            }
        }
    }

    fn install(&self, action: &gio::SimpleAction) {
        self.app.add_action(action);
        self.installed.borrow_mut().push(action.name().to_string());
    }

    fn add_action(&self, section: &gio::Menu, label: &str, shortcut: Option<&Shortcut>) {
        let name = format!("wtm-{}", slug(label));
        let action = gio::SimpleAction::new(&name, None);
        let on_select = slot(&wtm_toolkit::Handler::none());
        let s = on_select.clone();
        action.connect_activate(move |_, _| fire(&s, ()));
        self.install(&action);
        let item = gio::MenuItem::new(Some(label), Some(&format!("app.{name}")));
        if let Some(sc) = shortcut {
            let accel = accel(sc);
            item.set_attribute_value("accel", Some(&accel.to_variant()));
            // The key goes to an action of its own (see the module docs).
            let key_name = format!("wtm-key-{}", slug(label));
            let key_action = gio::SimpleAction::new(&key_name, None);
            let (items, item_name) = (self.items.clone(), slug(label));
            key_action.connect_activate(move |_, _| {
                wtm_toolkit::flush();
                let handler = {
                    let items = items.borrow();
                    items
                        .get(&item_name)
                        .filter(|st| st.enabled.get())
                        .map(|st| st.on_select.clone())
                };
                if let Some(h) = handler {
                    fire(&h, ());
                }
            });
            self.install(&key_action);
            self.app
                .set_accels_for_action(&format!("app.{key_name}"), &[accel.as_str()]);
        }
        section.append_item(&item);
        self.items.borrow_mut().insert(
            slug(label),
            ItemState {
                on_select,
                enabled: Cell::new(true),
                action,
            },
        );
    }

    fn add_standard(&self, section: &gio::Menu, s: Standard) {
        let Some((label, shown_accel)) = standard(s) else {
            return;
        };
        let name = format!("wtm-std-{}", slug(label));
        let action = gio::SimpleAction::new(&name, None);
        let app = self.app.clone();
        let quit = self.quit.clone();
        action.connect_activate(move |_, _| run_standard(&app, s, &quit));
        self.install(&action);
        let item = gio::MenuItem::new(Some(label), Some(&format!("app.{name}")));
        if let Some(a) = shown_accel {
            item.set_attribute_value("accel", Some(&a.to_variant()));
        }
        // Quit and Close Window are the app's; the editing keys belong to
        // whatever field has the keyboard, and an application accelerator
        // would take them from it.
        if matches!(s, Standard::Quit | Standard::CloseWindow) {
            if let Some(a) = shown_accel {
                self.app.set_accels_for_action(&format!("app.{name}"), &[a]);
            }
        }
        section.append_item(&item);
    }
}

fn focused(app: &gtk::Application) -> Option<gtk::Widget> {
    app.active_window()
        .and_then(|w| gtk::prelude::RootExt::focus(&w))
}

fn run_standard(app: &gtk::Application, s: Standard, quit: &Rc<dyn Fn()>) {
    let edit = |action: &str| {
        if let Some(w) = focused(app) {
            let _ = w.activate_action(action, None);
        }
    };
    match s {
        Standard::Quit => quit(),
        Standard::CloseWindow => {
            if let Some(w) = app.active_window() {
                w.close();
            }
        }
        Standard::About => {
            let about = gtk::AboutDialog::builder()
                .program_name("Worktree Manager")
                .version(env!("CARGO_PKG_VERSION"))
                .comments("Git worktrees across your repositories.")
                .logo_icon_name("folder-symbolic")
                .license_type(gtk::License::MitX11)
                .modal(true)
                .build();
            if let Some(w) = app.active_window() {
                about.set_transient_for(Some(&w));
            }
            crate::style::adopt(&about);
            about.present();
        }
        Standard::Undo => edit("text.undo"),
        Standard::Redo => edit("text.redo"),
        Standard::Cut => edit("clipboard.cut"),
        Standard::Copy => edit("clipboard.copy"),
        Standard::Paste => edit("clipboard.paste"),
        Standard::SelectAll => edit("selection.select-all"),
        Standard::Minimize => {
            if let Some(w) = app.active_window() {
                w.minimize();
            }
        }
        Standard::Zoom => {
            if let Some(w) = app.active_window() {
                if w.is_maximized() {
                    w.unmaximize();
                } else {
                    w.maximize();
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_use_ctrl_and_gdk_key_names() {
        assert_eq!(accel(&Shortcut::primary(KeyName::Char('n'))), "<Control>n");
        assert_eq!(
            accel(&Shortcut::primary(KeyName::Char(','))),
            "<Control>comma"
        );
        assert_eq!(
            accel(&Shortcut::primary(KeyName::Up).alt()),
            "<Control><Alt>Up"
        );
        let mut s = Shortcut::primary(KeyName::Char('Z'));
        s.shift = true;
        assert_eq!(accel(&s), "<Control><Shift>z");
    }

    #[test]
    fn labels_become_action_names() {
        assert_eq!(slug("New Worktree…"), "new-worktree");
        assert_eq!(slug("Add Repository…"), "add-repository");
        assert_eq!(slug("Settings…"), "settings");
        assert_eq!(slug("Move Repository Up"), "move-repository-up");
    }

    #[test]
    fn mac_only_items_are_skipped() {
        assert!(standard(Standard::Services).is_none());
        assert!(standard(Standard::Hide).is_none());
        assert!(standard(Standard::Quit).is_some());
        assert!(standard(Standard::Copy).is_some());
    }
}
