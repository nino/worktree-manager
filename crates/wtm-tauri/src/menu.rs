//! The menu bar, on macOS: the native build's menus (`wtm-ui`'s `menus.rs`),
//! item for item. Choosing one of the app's own items sends its id to the
//! page as a `menu` event, and the page does what its shortcut does; the
//! standard items (Copy, Hide, Minimize…) are AppKit's own.
//!
//! GNOME has no menu bar and the page already has every shortcut, so on
//! Linux and Windows there is none.

use tauri::menu::{
    Menu, MenuBuilder, MenuItem, MenuItemBuilder, SubmenuBuilder, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Wry};

use crate::wire::MenuState;

/// The items whose state follows the selection.
pub struct Items {
    new_worktree: MenuItem<Wry>,
    switch_branch: MenuItem<Wry>,
    move_up: MenuItem<Wry>,
    move_down: MenuItem<Wry>,
}

impl Items {
    pub fn set(&self, s: MenuState) {
        for (item, on) in [
            (&self.new_worktree, s.new_worktree),
            (&self.switch_branch, s.switch_branch),
            (&self.move_up, s.move_up),
            (&self.move_down, s.move_down),
        ] {
            let _ = item.set_enabled(on);
        }
    }
}

/// The ids the page knows (`menu.ts`).
pub const IDS: [&str; 8] = [
    "settings",
    "add-repo",
    "new-worktree",
    "switch-branch",
    "move-up",
    "move-down",
    "refresh",
    "find",
];

pub fn build(h: &AppHandle) -> tauri::Result<(Menu<Wry>, Items)> {
    let item = |id: &str, label: &str, key: &str, enabled: bool| {
        MenuItemBuilder::with_id(id, label)
            .accelerator(key)
            .enabled(enabled)
            .build(h)
    };
    // Off until the page reports a selection they apply to.
    let items = Items {
        new_worktree: item("new-worktree", "New Worktree…", "CmdOrCtrl+N", false)?,
        switch_branch: item("switch-branch", "Switch Branch…", "CmdOrCtrl+T", false)?,
        move_up: item("move-up", "Move Repository Up", "Alt+CmdOrCtrl+Up", false)?,
        move_down: item(
            "move-down",
            "Move Repository Down",
            "Alt+CmdOrCtrl+Down",
            false,
        )?,
    };
    let app_menu = SubmenuBuilder::new(h, "Worktree Manager")
        .about(None)
        .separator()
        .item(&item("settings", "Settings…", "CmdOrCtrl+,", true)?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(h, "File")
        .item(&item("add-repo", "Add Repository…", "CmdOrCtrl+O", true)?)
        .item(&items.new_worktree)
        .item(&items.switch_branch)
        .separator()
        // The keyboard's way to do what dragging a card does.
        .item(&items.move_up)
        .item(&items.move_down)
        .separator()
        .item(&item("refresh", "Refresh", "CmdOrCtrl+R", true)?)
        .separator()
        .close_window()
        .build()?;
    let edit = SubmenuBuilder::new(h, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item("find", "Find", "CmdOrCtrl+F", true)?)
        .build()?;
    // As the window menu, AppKit lists the open windows in it.
    let window = SubmenuBuilder::with_id(h, WINDOW_SUBMENU_ID, "Window")
        .minimize()
        .maximize_with_text("Zoom")
        .separator()
        .bring_all_to_front()
        .build()?;
    let menu = MenuBuilder::new(h)
        .items(&[&app_menu, &file, &edit, &window])
        .build()?;
    Ok((menu, items))
}
