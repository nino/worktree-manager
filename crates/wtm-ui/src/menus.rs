//! The menu bar. Items a toolkit implements itself (Cut, Hide, Minimize…)
//! are `Standard`; a toolkit without an application menu folds that menu's
//! items into the others.

use wtm_toolkit::{KeyName, Menu, MenuItem, MenuRole, Shortcut, Standard, ViewCx};

use crate::Msg;

/// What decides which items are enabled.
pub struct Enabled {
    pub new_worktree: bool,
    pub switch_branch: bool,
    pub move_up: bool,
    pub move_down: bool,
    pub check_for_updates: bool,
}

fn action(
    label: &str,
    key: Option<Shortcut>,
    enabled: bool,
    msg: Msg,
    v: &ViewCx<Msg>,
) -> MenuItem {
    MenuItem::Action {
        label: label.into(),
        shortcut: key,
        enabled,
        on_select: v.on(msg),
    }
}

fn key(c: char) -> Option<Shortcut> {
    Some(Shortcut::primary(KeyName::Char(c)))
}

pub fn menus(e: &Enabled, v: &ViewCx<Msg>) -> Vec<Menu> {
    use MenuItem::{Separator, Standard as Std};
    let mut app = vec![Std(Standard::About)];
    if e.check_for_updates {
        app.push(action(
            "Check for Updates…",
            None,
            true,
            Msg::CheckForUpdates,
            v,
        ));
    }
    app.extend([
        Separator,
        action("Settings…", key(','), true, Msg::OpenSettings, v),
        Separator,
        Std(Standard::Services),
        Separator,
        Std(Standard::Hide),
        Std(Standard::HideOthers),
        Std(Standard::ShowAll),
        Separator,
        Std(Standard::Quit),
    ]);
    vec![
        Menu {
            role: MenuRole::App,
            title: "Worktree Manager".into(),
            items: app,
        },
        Menu {
            role: MenuRole::File,
            title: "File".into(),
            items: vec![
                action("Add Repository…", key('o'), true, Msg::AddRepo, v),
                action(
                    "New Worktree…",
                    key('n'),
                    e.new_worktree,
                    Msg::NewWorktree,
                    v,
                ),
                action(
                    "Switch Branch…",
                    key('t'),
                    e.switch_branch,
                    Msg::SwitchBranch,
                    v,
                ),
                Separator,
                // The keyboard's way to do what dragging a card does.
                action(
                    "Move Repository Up",
                    Some(Shortcut::primary(KeyName::Up).alt()),
                    e.move_up,
                    Msg::MoveRepo { up: true },
                    v,
                ),
                action(
                    "Move Repository Down",
                    Some(Shortcut::primary(KeyName::Down).alt()),
                    e.move_down,
                    Msg::MoveRepo { up: false },
                    v,
                ),
                Separator,
                action("Refresh", key('r'), true, Msg::Refresh, v),
                Separator,
                Std(Standard::CloseWindow),
            ],
        },
        Menu {
            role: MenuRole::Edit,
            title: "Edit".into(),
            items: vec![
                Std(Standard::Undo),
                Std(Standard::Redo),
                Separator,
                Std(Standard::Cut),
                Std(Standard::Copy),
                Std(Standard::Paste),
                Std(Standard::SelectAll),
                Separator,
                action("Find", key('f'), true, Msg::FocusSearch, v),
            ],
        },
        Menu {
            role: MenuRole::Window,
            title: "Window".into(),
            items: vec![
                Std(Standard::Minimize),
                Std(Standard::Zoom),
                Separator,
                Std(Standard::BringAllToFront),
            ],
        },
    ]
}
