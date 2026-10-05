//! The view's menus as the menu bar. The app's own items target the
//! controller, which runs their handlers by tag; the standard ones go to the
//! responder chain under AppKit's own selectors, which text fields in sheets
//! rely on.
//!
//! The bar is rebuilt only when the menus change shape. An item's enabled
//! state is read when AppKit validates it, so it is never rebuilt for that.

use std::cell::{Cell, RefCell};

use dispatch2::DispatchQueue;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSInteger;
use wtm_toolkit::{Handler, KeyName, Menu, MenuItem, MenuRole, Shortcut, Standard};

use crate::util::ns;

thread_local! {
    /// What the bar was built from: everything but handlers and enabled
    /// states, which are taken from each render.
    static SHAPE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Each action item's handler and enabled state, by tag.
    static ACTIONS: RefCell<Vec<(Handler, bool)>> = const { RefCell::new(Vec::new()) };
    /// AppKit is validating the bar: a new one waits for the next turn.
    static VALIDATING: Cell<bool> = const { Cell::new(false) };
    /// The latest menus that waited for validation to finish.
    static DEFERRED: RefCell<Option<Vec<Menu>>> = const { RefCell::new(None) };
}

/// Whether the item with `tag` is enabled, after running whatever the
/// program still has queued. That can render, and a render that changes the
/// menus' shape would replace the bar AppKit is in the middle of tracking;
/// the new one is put up on the next turn instead.
pub fn validate(tag: NSInteger, flush: impl FnOnce()) -> bool {
    VALIDATING.with(|v| v.set(true));
    flush();
    VALIDATING.with(|v| v.set(false));
    enabled(tag)
}

/// Whether the item with `tag` is enabled, as last rendered.
pub fn enabled(tag: NSInteger) -> bool {
    ACTIONS.with(|a| a.borrow().get(tag as usize).is_some_and(|(_, e)| *e))
}

/// Run the handler of the item with `tag`.
pub fn perform(tag: NSInteger) {
    let handler = ACTIONS.with(|a| a.borrow().get(tag as usize).map(|(h, _)| h.clone()));
    if let Some(h) = handler {
        h.call(());
    }
}

/// The menus without what changes from render to render.
fn shape(menus: &[Menu]) -> String {
    let mut s = String::new();
    for m in menus {
        s.push_str(&format!("{:?}:{}[", m.role, m.title));
        for i in &m.items {
            match i {
                MenuItem::Action {
                    label, shortcut, ..
                } => s.push_str(&format!("{label}{shortcut:?};")),
                MenuItem::Separator => s.push_str("-;"),
                MenuItem::Standard(st) => s.push_str(&format!("{st:?};")),
            }
        }
        s.push(']');
    }
    s
}

pub fn render(menus: &[Menu], controller: &AnyObject, mtm: MainThreadMarker) {
    let actions = menus
        .iter()
        .flat_map(|m| &m.items)
        .filter_map(|i| match i {
            MenuItem::Action {
                enabled, on_select, ..
            } => Some((on_select.clone(), *enabled)),
            _ => None,
        })
        .collect();
    ACTIONS.with(|a| *a.borrow_mut() = actions);
    let shape = shape(menus);
    if SHAPE.with(|s| s.borrow().as_deref() == Some(shape.as_str())) {
        return;
    }
    if VALIDATING.with(Cell::get) {
        if DEFERRED.with(|d| d.replace(Some(menus.to_vec()))).is_none() {
            DispatchQueue::main().exec_async(|| {
                let Some(menus) = DEFERRED.with(|d| d.take()) else {
                    return;
                };
                let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                let app = NSApplication::sharedApplication(mtm);
                if let Some(controller) = app.delegate() {
                    render(&menus, controller.as_ref(), mtm);
                }
            });
        }
        return;
    }
    DEFERRED.with(|d| d.borrow_mut().take());
    SHAPE.with(|s| *s.borrow_mut() = Some(shape));
    install(menus, controller, mtm);
}

fn item(
    title: &str,
    action: Option<Sel>,
    key: &str,
    target: Option<&AnyObject>,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let i = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &ns(title), action, &ns(key))
    };
    if let Some(t) = target {
        unsafe { i.setTarget(Some(t)) };
    }
    i
}

fn key_equivalent(s: &Shortcut) -> (String, NSEventModifierFlags) {
    let key = match s.key {
        KeyName::Char(c) => c.to_string(),
        KeyName::Up => "\u{F700}".into(),
        KeyName::Down => "\u{F701}".into(),
    };
    let mut mods = NSEventModifierFlags::Command;
    if s.shift {
        mods |= NSEventModifierFlags::Shift;
    }
    if s.alt {
        mods |= NSEventModifierFlags::Option;
    }
    (key, mods)
}

/// A standard item: its title, AppKit's selector for it, its key and
/// modifiers.
fn standard(s: Standard, app_name: &str) -> (String, Sel, &'static str, NSEventModifierFlags) {
    let cmd = NSEventModifierFlags::Command;
    match s {
        Standard::About => (
            format!("About {app_name}"),
            sel!(orderFrontStandardAboutPanel:),
            "",
            cmd,
        ),
        Standard::Services => unreachable!("built as a submenu by `install`"),
        Standard::Hide => (format!("Hide {app_name}"), sel!(hide:), "h", cmd),
        Standard::HideOthers => (
            "Hide Others".into(),
            sel!(hideOtherApplications:),
            "h",
            cmd | NSEventModifierFlags::Option,
        ),
        Standard::ShowAll => ("Show All".into(), sel!(unhideAllApplications:), "", cmd),
        Standard::Quit => (format!("Quit {app_name}"), sel!(terminate:), "q", cmd),
        Standard::Undo => ("Undo".into(), sel!(undo:), "z", cmd),
        Standard::Redo => (
            "Redo".into(),
            sel!(redo:),
            "z",
            cmd | NSEventModifierFlags::Shift,
        ),
        Standard::Cut => ("Cut".into(), sel!(cut:), "x", cmd),
        Standard::Copy => ("Copy".into(), sel!(copy:), "c", cmd),
        Standard::Paste => ("Paste".into(), sel!(paste:), "v", cmd),
        Standard::SelectAll => ("Select All".into(), sel!(selectAll:), "a", cmd),
        Standard::CloseWindow => ("Close Window".into(), sel!(performClose:), "w", cmd),
        Standard::Minimize => ("Minimize".into(), sel!(performMiniaturize:), "m", cmd),
        Standard::Zoom => ("Zoom".into(), sel!(performZoom:), "", cmd),
        Standard::BringAllToFront => ("Bring All to Front".into(), sel!(arrangeInFront:), "", cmd),
    }
}

fn install(menus: &[Menu], controller: &AnyObject, mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    let bar = NSMenu::new(mtm);
    let app_name = menus
        .iter()
        .find(|m| m.role == MenuRole::App)
        .map_or("", |m| m.title.as_str())
        .to_string();
    let mut tag: NSInteger = 0;
    for m in menus {
        let holder = item(&m.title, None, "", None, mtm);
        let menu = NSMenu::initWithTitle(mtm.alloc(), &ns(&m.title));
        holder.setSubmenu(Some(&menu));
        for i in &m.items {
            match i {
                MenuItem::Action {
                    label, shortcut, ..
                } => {
                    let (key, mods) = shortcut
                        .as_ref()
                        .map(key_equivalent)
                        .unwrap_or_else(|| (String::new(), NSEventModifierFlags::Command));
                    let it = item(label, Some(sel!(menuAction:)), &key, Some(controller), mtm);
                    it.setKeyEquivalentModifierMask(mods);
                    it.setTag(tag);
                    tag += 1;
                    menu.addItem(&it);
                }
                MenuItem::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                MenuItem::Standard(Standard::Services) => {
                    let services = NSMenu::new(mtm);
                    let it = item("Services", None, "", None, mtm);
                    it.setSubmenu(Some(&services));
                    menu.addItem(&it);
                    app.setServicesMenu(Some(&services));
                }
                MenuItem::Standard(s) => {
                    let (title, action, key, mods) = standard(*s, &app_name);
                    let it = item(&title, Some(action), key, None, mtm);
                    it.setKeyEquivalentModifierMask(mods);
                    menu.addItem(&it);
                }
            }
        }
        bar.addItem(&holder);
        if m.role == MenuRole::Window {
            app.setWindowsMenu(Some(&menu));
        }
    }
    app.setMainMenu(Some(&bar));
}
