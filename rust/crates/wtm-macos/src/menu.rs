//! The menu bar. Every item targets the controller (or the responder chain for
//! the standard Edit actions, which text fields in sheets rely on).

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};

use crate::util::ns;

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

fn submenu(title: &str, mtm: MainThreadMarker) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
    let holder = item(title, None, "", None, mtm);
    let menu = NSMenu::initWithTitle(mtm.alloc(), &ns(title));
    holder.setSubmenu(Some(&menu));
    (holder, menu)
}

pub fn install(app: &NSApplication, controller: &AnyObject, mtm: MainThreadMarker) {
    let bar = NSMenu::new(mtm);

    // Application menu.
    let (app_item, app_menu) = submenu("Worktree Manager", mtm);
    app_menu.addItem(&item(
        "About Worktree Manager",
        Some(sel!(orderFrontStandardAboutPanel:)),
        "",
        None,
        mtm,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(
        "Settings…",
        Some(sel!(openSettings:)),
        ",",
        Some(controller),
        mtm,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    let services = NSMenu::new(mtm);
    let services_item = item("Services", None, "", None, mtm);
    services_item.setSubmenu(Some(&services));
    app_menu.addItem(&services_item);
    app.setServicesMenu(Some(&services));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(
        "Hide Worktree Manager",
        Some(sel!(hide:)),
        "h",
        None,
        mtm,
    ));
    let hide_others = item(
        "Hide Others",
        Some(sel!(hideOtherApplications:)),
        "h",
        None,
        mtm,
    );
    hide_others
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    app_menu.addItem(&hide_others);
    app_menu.addItem(&item(
        "Show All",
        Some(sel!(unhideAllApplications:)),
        "",
        None,
        mtm,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(
        "Quit Worktree Manager",
        Some(sel!(terminate:)),
        "q",
        None,
        mtm,
    ));
    bar.addItem(&app_item);

    // File.
    let (file_item, file) = submenu("File", mtm);
    file.addItem(&item(
        "Add Repository…",
        Some(sel!(addRepo:)),
        "o",
        Some(controller),
        mtm,
    ));
    file.addItem(&item(
        "New Worktree…",
        Some(sel!(newWorktree:)),
        "n",
        Some(controller),
        mtm,
    ));
    file.addItem(&NSMenuItem::separatorItem(mtm));
    file.addItem(&item(
        "Refresh",
        Some(sel!(refresh:)),
        "r",
        Some(controller),
        mtm,
    ));
    file.addItem(&NSMenuItem::separatorItem(mtm));
    file.addItem(&item(
        "Close Window",
        Some(sel!(performClose:)),
        "w",
        None,
        mtm,
    ));
    bar.addItem(&file_item);

    // Edit: standard responder-chain actions so text fields work.
    let (edit_item, edit) = submenu("Edit", mtm);
    edit.addItem(&item("Undo", Some(sel!(undo:)), "z", None, mtm));
    let redo = item("Redo", Some(sel!(redo:)), "z", None, mtm);
    redo.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    edit.addItem(&redo);
    edit.addItem(&NSMenuItem::separatorItem(mtm));
    edit.addItem(&item("Cut", Some(sel!(cut:)), "x", None, mtm));
    edit.addItem(&item("Copy", Some(sel!(copy:)), "c", None, mtm));
    edit.addItem(&item("Paste", Some(sel!(paste:)), "v", None, mtm));
    edit.addItem(&item("Select All", Some(sel!(selectAll:)), "a", None, mtm));
    edit.addItem(&NSMenuItem::separatorItem(mtm));
    edit.addItem(&item(
        "Find",
        Some(sel!(focusSearch:)),
        "f",
        Some(controller),
        mtm,
    ));
    bar.addItem(&edit_item);

    // Window.
    let (window_item, window) = submenu("Window", mtm);
    window.addItem(&item(
        "Minimize",
        Some(sel!(performMiniaturize:)),
        "m",
        None,
        mtm,
    ));
    window.addItem(&item("Zoom", Some(sel!(performZoom:)), "", None, mtm));
    window.addItem(&NSMenuItem::separatorItem(mtm));
    window.addItem(&item(
        "Bring All to Front",
        Some(sel!(arrangeInFront:)),
        "",
        None,
        mtm,
    ));
    bar.addItem(&window_item);
    app.setWindowsMenu(Some(&window));

    app.setMainMenu(Some(&bar));
}
