//! The backend: the main window (header bar, notice bar, the list or the
//! empty state, the activity spinner), and the dialogs, popover, panels and
//! menus around it. `render` brings all of them in line with a [`View`];
//! `perform` carries out the effects.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gio, glib};

use wtm_toolkit::{
    host_event, Axis, Backend, Effect, Frame, HostEvent, Id, MainWindow, PickFolders, ToolItem,
    View,
};

use crate::dialogs::DialogHost;
use crate::elements::Node;
use crate::list::ListW;
use crate::menus::MenuHost;
use crate::panels::PanelHost;
use crate::popover::PickerHost;
use crate::screenshot::Shot;
use crate::style;
use crate::util::{fire, put, quietly, set_hint, show, slot, Slot};

/// The window's size when nothing was remembered.
const DEFAULT_SIZE: (i32, i32) = (940, 720);

pub struct GtkBackend {
    pub(crate) inner: Rc<Inner>,
}

pub(crate) struct Inner {
    pub app: Option<gtk::Application>,
    pub window: RefCell<Option<gtk::Window>>,
    pub main: RefCell<Option<MainW>>,
    pub dialogs: DialogHost,
    pub picker: PickerHost,
    pub panels: PanelHost,
    menus: RefCell<Option<MenuHost>>,
    /// The last view rendered, for the screenshot driver.
    pub last: RefCell<Option<View>>,
    pub shot: Option<Shot>,
    quitting: Cell<bool>,
    pub me: Weak<Inner>,
}

impl GtkBackend {
    pub fn new(app: Option<gtk::Application>) -> GtkBackend {
        let inner = Rc::new_cyclic(|me| Inner {
            app,
            window: RefCell::new(None),
            main: RefCell::new(None),
            dialogs: DialogHost::default(),
            picker: PickerHost::default(),
            panels: PanelHost::default(),
            menus: RefCell::new(None),
            last: RefCell::new(None),
            shot: Shot::from_env(),
            quitting: Cell::new(false),
            me: me.clone(),
        });
        if let Some(app) = &inner.app {
            let me = inner.me.clone();
            let quit: Rc<dyn Fn()> = Rc::new(move || {
                if let Some(i) = me.upgrade() {
                    i.quit();
                }
            });
            *inner.menus.borrow_mut() = Some(MenuHost::new(app, quit));
            let me = inner.me.clone();
            app.connect_shutdown(move |_| {
                if let Some(i) = me.upgrade() {
                    i.will_quit();
                }
            });
        }
        GtkBackend { inner }
    }
}

impl Backend for GtkBackend {
    fn render(&self, view: &View) {
        self.inner.render(view);
    }

    fn perform(&self, effect: Effect) {
        self.inner.perform(effect);
    }
}

impl Inner {
    pub fn render(&self, view: &View) {
        quietly(|| {
            let window = self.ensure_window(&view.window);
            if let Some(m) = self.menus.borrow().as_ref() {
                m.render(&view.menus);
            }
            if let Some(main) = self.main.borrow().as_ref() {
                main.render(&view.window);
                self.picker.render(view.popover.as_ref(), &main.list);
            }
            self.dialogs.render(&view.dialogs, &window);
            self.panels.render(&view.panels, &window, self.app.as_ref());
        });
        *self.last.borrow_mut() = Some(view.clone());
        if let Some(shot) = &self.shot {
            shot.after_render(self);
        }
    }

    pub fn perform(&self, effect: Effect) {
        match effect {
            Effect::After(delay, then) => {
                glib::timeout_add_local_once(delay, move || then.call(()));
            }
            Effect::Copy(text) => {
                if let Some(d) = gtk::gdk::Display::default() {
                    d.clipboard().set_text(&text);
                }
            }
            Effect::PickFolders(p) => self.pick_folders(p),
            Effect::FocusSearch => {
                if let Some(s) = self.main.borrow().as_ref().and_then(|m| m.search.clone()) {
                    quietly(|| s.grab_focus());
                }
            }
            Effect::FocusList => {
                if let Some(m) = self.main.borrow().as_ref() {
                    quietly(|| m.list.focus());
                }
            }
            Effect::ScrollTo(offset) => {
                if let Some(m) = self.main.borrow().as_ref() {
                    m.list.scroll_to(offset);
                }
            }
            Effect::RevealSelection => {
                if let Some(m) = self.main.borrow().as_ref() {
                    m.list.reveal_selection();
                }
            }
            Effect::PresentPanel(key) => self.panels.present(&key),
        }
    }

    /// The main window, made by the first render: its size comes from the
    /// view, which knows it only once the screens are known.
    fn ensure_window(&self, view: &MainWindow) -> gtk::Window {
        if let Some(w) = self.window.borrow().as_ref() {
            if w.title().as_deref() != Some(view.title.as_str()) {
                w.set_title(Some(&view.title));
            }
            return w.clone();
        }
        let window: gtk::Window = match &self.app {
            Some(app) => gtk::ApplicationWindow::new(app).upcast(),
            None => gtk::Window::new(),
        };
        window.set_title(Some(&view.title));
        // GTK 4 cannot place a window; only the size of the frame is used.
        let (w, h) = view
            .frame
            .map(|f| (f.width.round() as i32, f.height.round() as i32))
            .unwrap_or(DEFAULT_SIZE);
        window.set_default_size(w.max(view.min_size.0 as i32), h.max(view.min_size.1 as i32));
        window.set_size_request(view.min_size.0 as i32, view.min_size.1 as i32);
        style::adopt(&window);

        let main = MainW::new(view, self.menus.borrow().as_ref());
        window.set_titlebar(Some(&main.header));
        window.set_child(Some(&main.root));

        let me = self.me.clone();
        window.connect_close_request(move |_| {
            if let Some(i) = me.upgrade() {
                i.quit();
            }
            glib::Propagation::Stop
        });
        window.connect_is_active_notify(|w| {
            if w.is_active() {
                host_event(HostEvent::Activated);
            }
        });
        let report = |w: &gtk::Window| {
            if w.is_maximized() || w.is_fullscreen() {
                return;
            }
            let (width, height) = w.default_size();
            host_event(HostEvent::FrameChanged(Frame {
                x: 0.0,
                y: 0.0,
                width: width as f64,
                height: height as f64,
            }));
        };
        window.connect_default_width_notify(report);
        window.connect_default_height_notify(report);

        *self.main.borrow_mut() = Some(main);
        *self.window.borrow_mut() = Some(window.clone());
        window.present();
        window
    }

    fn pick_folders(&self, p: PickFolders) {
        let parent = p
            .parent
            .as_deref()
            .and_then(|k| self.panels.window(k))
            .or_else(|| self.window.borrow().clone());
        let dialog = gtk::FileDialog::builder()
            .title(p.title.as_str())
            .modal(true)
            .build();
        let done = p.on_done.clone();
        if p.multiple {
            dialog.select_multiple_folders(parent.as_ref(), gio::Cancellable::NONE, move |r| {
                let paths: Vec<PathBuf> = match r {
                    Ok(list) => (0..list.n_items())
                        .filter_map(|i| list.item(i))
                        .filter_map(|o| o.downcast::<gio::File>().ok())
                        .filter_map(|f| f.path())
                        .collect(),
                    Err(_) => Vec::new(),
                };
                done.call(paths);
            });
        } else {
            dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |r| {
                let paths = r.ok().and_then(|f| f.path()).into_iter().collect();
                done.call(paths);
            });
        }
    }

    /// Tell the program, let it write what it must, and go.
    pub fn quit(&self) {
        self.will_quit();
        match &self.app {
            Some(app) => app.quit(),
            None => {
                if let Some(w) = self.window.borrow().as_ref() {
                    w.destroy();
                }
            }
        }
    }

    fn will_quit(&self) {
        if self.quitting.replace(true) {
            return;
        }
        host_event(HostEvent::WillQuit);
        wtm_toolkit::flush();
    }
}

// MARK: The main window

struct ToolW {
    id: Id,
    widget: gtk::Widget,
    slot: Option<Slot<()>>,
    search: Option<(gtk::SearchEntry, Slot<String>)>,
}

pub(crate) struct MainW {
    header: gtk::HeaderBar,
    root: gtk::Box,
    tools: RefCell<Vec<ToolW>>,
    tool_shape: RefCell<Vec<String>>,
    pub search: Option<gtk::SearchEntry>,
    notice_box: gtk::Box,
    notice: RefCell<Option<Node>>,
    stack: gtk::Stack,
    empty_box: gtk::Box,
    empty: RefCell<Option<Node>>,
    pub list: Rc<ListW>,
    activity: gtk::Spinner,
}

fn tool_shape(items: &[ToolItem]) -> Vec<String> {
    items
        .iter()
        .map(|t| match t {
            ToolItem::Button { id, .. } => format!("b:{id}"),
            ToolItem::Search { id, .. } => format!("s:{id}"),
            ToolItem::Flex => "flex".into(),
        })
        .collect()
}

impl MainW {
    fn new(view: &MainWindow, menus: Option<&MenuHost>) -> MainW {
        let header = gtk::HeaderBar::new();
        let mut tools = Vec::new();
        let mut search = None;
        let mut after_flex = Vec::new();
        let mut flexed = false;
        for item in &view.toolbar {
            let t = match item {
                ToolItem::Flex => {
                    flexed = true;
                    continue;
                }
                ToolItem::Button {
                    id,
                    label,
                    icon,
                    on_press,
                } => {
                    let b = gtk::Button::new();
                    b.set_child(Some(&crate::draw::icon(*icon, 16)));
                    set_hint(&b, label, true);
                    let s = slot(on_press);
                    crate::util::on_click(&b, &s);
                    ToolW {
                        id,
                        widget: b.upcast(),
                        slot: Some(s),
                        search: None,
                    }
                }
                ToolItem::Search {
                    id,
                    value,
                    placeholder,
                    on_change,
                } => {
                    let e = gtk::SearchEntry::new();
                    e.set_placeholder_text(Some(placeholder));
                    e.set_width_chars(24);
                    e.set_text(value);
                    let s = slot(on_change);
                    let s2 = s.clone();
                    e.connect_changed(move |e| fire(&s2, e.text().to_string()));
                    // Escape clears the search, as it does in a search field
                    // everywhere else.
                    e.connect_stop_search(|e| e.set_text(""));
                    search = Some(e.clone());
                    ToolW {
                        id,
                        widget: e.clone().upcast(),
                        slot: None,
                        search: Some((e, s)),
                    }
                }
            };
            if flexed {
                after_flex.push(t.widget.clone());
            } else {
                header.pack_start(&t.widget);
            }
            tools.push(t);
        }
        if let Some(m) = menus {
            header.pack_end(&m.button);
        }
        for w in after_flex.iter().rev() {
            header.pack_end(w);
        }

        let list = ListW::new();
        let notice_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        notice_box.add_css_class("wtm-notice");
        notice_box.set_visible(false);
        let empty_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        empty_box.add_css_class("wtm-empty");
        empty_box.set_valign(gtk::Align::Center);
        empty_box.set_halign(gtk::Align::Center);
        let stack = gtk::Stack::new();
        stack.add_named(&list.scroller, Some("list"));
        stack.add_named(&empty_box, Some("empty"));
        stack.set_vexpand(true);
        let activity = gtk::Spinner::new();
        activity.add_css_class("wtm-activity");
        activity.set_halign(gtk::Align::End);
        activity.set_valign(gtk::Align::End);
        activity.set_can_target(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&activity);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&notice_box);
        root.append(&overlay);

        MainW {
            header,
            root,
            tools: RefCell::new(tools),
            tool_shape: RefCell::new(tool_shape(&view.toolbar)),
            search,
            notice_box,
            notice: RefCell::new(None),
            stack,
            empty_box,
            empty: RefCell::new(None),
            list,
            activity,
        }
    }

    fn render(&self, view: &MainWindow) {
        if *self.tool_shape.borrow() != tool_shape(&view.toolbar) {
            log::warn!("the toolbar changed shape after launch; only its handlers follow");
        }
        let tools = self.tools.borrow();
        for item in &view.toolbar {
            match item {
                ToolItem::Button {
                    id,
                    label,
                    on_press,
                    ..
                } => {
                    if let Some(t) = tools.iter().find(|t| t.id == *id) {
                        if let Some(s) = &t.slot {
                            put(s, on_press);
                        }
                        if t.widget.tooltip_text().as_deref() != Some(label.as_str()) {
                            set_hint(&t.widget, label, true);
                        }
                    }
                }
                ToolItem::Search {
                    id,
                    value,
                    placeholder,
                    on_change,
                } => {
                    if let Some((e, s)) = tools
                        .iter()
                        .find(|t| t.id == *id)
                        .and_then(|t| t.search.as_ref())
                    {
                        put(s, on_change);
                        if !crate::util::has_keyboard(e) && e.text() != *value {
                            e.set_text(value);
                        }
                        if e.placeholder_text().as_deref() != Some(placeholder.as_str()) {
                            e.set_placeholder_text(Some(placeholder));
                        }
                    }
                }
                ToolItem::Flex => {}
            }
        }

        patch_slot(
            &self.notice_box,
            &self.notice,
            view.notice.as_ref(),
            Axis::Vertical,
        );
        show(&self.notice_box, view.notice.is_some());
        patch_slot(
            &self.empty_box,
            &self.empty,
            view.empty.as_ref(),
            Axis::Vertical,
        );
        let page = if view.empty.is_some() {
            "empty"
        } else {
            "list"
        };
        if self.stack.visible_child_name().as_deref() != Some(page) {
            self.stack.set_visible_child_name(page);
        }

        self.list.render(&view.list);
        crate::elements::patch_spinner(&self.activity, &view.activity);
    }
}

/// Patch or replace the one element a container holds.
fn patch_slot(
    container: &gtk::Box,
    slot: &RefCell<Option<Node>>,
    el: Option<&wtm_toolkit::Element>,
    axis: Axis,
) {
    let mut cur = slot.borrow_mut();
    if let (Some(node), Some(el)) = (cur.as_mut(), el) {
        if node.patch(el, axis) {
            return;
        }
    }
    if let Some(old) = cur.take().and_then(|n| n.widget) {
        container.remove(&old);
    }
    if let Some(el) = el {
        let node = Node::build(el, axis);
        if let Some(w) = &node.widget {
            container.append(w);
        }
        *cur = Some(node);
    }
}
