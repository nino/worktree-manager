//! The menu bar and its keyboard shortcuts.
//!
//! Windows has no application menu, so `MenuRole::App`'s items are folded
//! into the others the way Windows apps place them: Settings… at the end of
//! File above Exit, About and update checks under Help. Items macOS
//! implements itself with no Windows counterpart (Services, Hide, Bring All
//! to Front) are left out.

use std::cell::Cell;
use std::collections::HashMap;

use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::{Handler, KeyName, Menu, MenuItem, MenuRole, Shortcut, Standard};

use crate::app::{later, main_window, nested, with_state};
use crate::util::*;

/// Command ids: actions from here up, one per label for the life of the
/// process (see `Menus::ids`).
const FIRST_ACTION: u32 = 1000;
/// Standard items: this plus their index in `STANDARD`.
const FIRST_STANDARD: u32 = 900;

const STANDARD: [Standard; 16] = [
    Standard::About,
    Standard::Services,
    Standard::Hide,
    Standard::HideOthers,
    Standard::ShowAll,
    Standard::Quit,
    Standard::Undo,
    Standard::Redo,
    Standard::Cut,
    Standard::Copy,
    Standard::Paste,
    Standard::SelectAll,
    Standard::CloseWindow,
    Standard::Minimize,
    Standard::Zoom,
    Standard::BringAllToFront,
];

#[derive(Clone)]
enum Entry {
    Action {
        label: String,
        shortcut: Option<Shortcut>,
        enabled: bool,
        handler: Handler,
    },
    Standard(Standard),
    Separator,
}

struct Folded {
    title: &'static str,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct Menus {
    shape: Vec<String>,
    bar: Option<HMENU>,
    accel: Option<HACCEL>,
    /// Each action's handler and whether it is enabled, by command id.
    actions: HashMap<u32, (Handler, bool)>,
    /// The command id of each action, by its menu and label. An id never
    /// changes, so a command from a bar that has since been rebuilt (it
    /// is posted after the menu closes) still finds its action.
    ids: HashMap<String, u32>,
    /// The bar no longer matches the view, but the menu was open: it is
    /// rebuilt when the menu closes.
    stale: bool,
}

thread_local! {
    /// The user is in the menu bar. Destroying the menu being tracked
    /// leaves Windows tracking freed handles, so a rebuild waits.
    static TRACKING: Cell<bool> = const { Cell::new(false) };
}

/// The menu loop started (`true`) or ended.
pub fn set_tracking(on: bool) {
    TRACKING.with(|t| t.set(on));
}

impl Menus {
    pub fn accel(&self) -> Option<HACCEL> {
        self.accel
    }

    /// A rebuild was put off while the menu was open.
    pub fn stale(&self) -> bool {
        self.stale
    }

    fn id_of(&mut self, menu: &str, label: &str) -> u32 {
        let next = FIRST_ACTION + self.ids.len() as u32;
        *self.ids.entry(format!("{menu}\t{label}")).or_insert(next)
    }

    pub fn render(&mut self, main: HWND, menus: &[Menu]) {
        let folded = fold(menus);
        let shape: Vec<String> = folded
            .iter()
            .flat_map(|m| {
                std::iter::once(m.title.to_string()).chain(m.entries.iter().map(|e| match e {
                    Entry::Action {
                        label, shortcut, ..
                    } => format!("{label}\t{shortcut:?}"),
                    Entry::Standard(s) => format!("{s:?}"),
                    Entry::Separator => "-".into(),
                }))
            })
            .collect();
        let mut actions = HashMap::new();
        for m in &folded {
            for e in &m.entries {
                if let Entry::Action {
                    label,
                    handler,
                    enabled,
                    ..
                } = e
                {
                    let id = self.id_of(m.title, label);
                    actions.insert(id, (handler.clone(), *enabled));
                }
            }
        }
        if shape != self.shape {
            if TRACKING.with(|t| t.get()) {
                self.stale = true;
            } else {
                self.rebuild(main, &folded);
                self.shape = shape;
                self.stale = false;
            }
        } else if let Some(bar) = self.bar {
            for (id, (_, now)) in &actions {
                if self.actions.get(id).map(|a| a.1) != Some(*now) {
                    let flag = if *now { MF_ENABLED } else { MF_GRAYED };
                    unsafe {
                        let _ = EnableMenuItem(bar, *id, MF_BYCOMMAND | flag);
                    }
                }
            }
        }
        self.actions = actions;
    }

    fn rebuild(&mut self, main: HWND, folded: &[Folded]) {
        let mut accels = Vec::new();
        unsafe {
            let Ok(bar) = CreateMenu() else { return };
            for m in folded {
                let Ok(popup) = CreatePopupMenu() else {
                    continue;
                };
                for e in &m.entries {
                    match e {
                        Entry::Separator => {
                            let _ = AppendMenuW(popup, MF_SEPARATOR, 0, None);
                        }
                        Entry::Action {
                            label,
                            shortcut,
                            enabled,
                            ..
                        } => {
                            let next = self.id_of(m.title, label);
                            let mut text = label.replace('&', "&&");
                            if let Some(s) = shortcut {
                                text.push('\t');
                                text.push_str(&shortcut_text(s));
                                if let Some(a) = accel(s, next) {
                                    accels.push(a);
                                }
                            }
                            let flags = MF_STRING | if *enabled { MF_ENABLED } else { MF_GRAYED };
                            let _ = AppendMenuW(popup, flags, next as usize, &HSTRING::from(text));
                        }
                        Entry::Standard(s) => {
                            let Some(text) = standard_label(*s) else {
                                continue;
                            };
                            let id = FIRST_STANDARD
                                + STANDARD.iter().position(|x| x == s).unwrap_or(0) as u32;
                            let _ =
                                AppendMenuW(popup, MF_STRING, id as usize, &HSTRING::from(text));
                        }
                    }
                }
                let _ = AppendMenuW(bar, MF_POPUP, popup.0 as usize, &HSTRING::from(m.title));
            }
            let _ = SetMenu(main, Some(bar));
            if let Some(old) = self.bar.replace(bar) {
                let _ = DestroyMenu(old);
            }
            let _ = DrawMenuBar(main);
            if let Some(old) = self.accel.take() {
                let _ = DestroyAcceleratorTable(old);
            }
            self.accel = CreateAcceleratorTableW(&accels).ok();
        }
    }
}

fn fold(menus: &[Menu]) -> Vec<Folded> {
    let items = |role: MenuRole| -> Vec<MenuItem> {
        menus
            .iter()
            .filter(|m| m.role == role)
            .flat_map(|m| m.items.iter().cloned())
            .collect()
    };
    let entry = |i: &MenuItem| -> Entry {
        match i {
            MenuItem::Action {
                label,
                shortcut,
                enabled,
                on_select,
            } => Entry::Action {
                label: label.clone(),
                shortcut: *shortcut,
                enabled: *enabled,
                handler: on_select.clone(),
            },
            MenuItem::Separator => Entry::Separator,
            MenuItem::Standard(s) => Entry::Standard(*s),
        }
    };
    let is_settings = |i: &MenuItem| matches!(i, MenuItem::Action { label, .. } if label.starts_with("Settings") || label.starts_with("Preferences"));
    let app = items(MenuRole::App);

    let mut file: Vec<Entry> = items(MenuRole::File).iter().map(entry).collect();
    file.push(Entry::Separator);
    file.extend(app.iter().filter(|i| is_settings(i)).map(entry));
    file.push(Entry::Separator);
    file.push(Entry::Standard(Standard::Quit));

    let edit: Vec<Entry> = items(MenuRole::Edit).iter().map(entry).collect();
    let window: Vec<Entry> = items(MenuRole::Window).iter().map(entry).collect();

    // Help: everything else the app menu had (update checks), then About.
    let mut help: Vec<Entry> = app
        .iter()
        .filter(|i| matches!(i, MenuItem::Action { .. }) && !is_settings(i))
        .map(entry)
        .collect();
    if app
        .iter()
        .any(|i| matches!(i, MenuItem::Standard(Standard::About)))
    {
        help.push(Entry::Separator);
        help.push(Entry::Standard(Standard::About));
    }

    [
        ("&File", file),
        ("&Edit", edit),
        ("&Window", window),
        ("&Help", help),
    ]
    .into_iter()
    .map(|(title, entries)| Folded {
        title,
        entries: tidy(entries),
    })
    .filter(|m| !m.entries.is_empty())
    .collect()
}

/// Drop the items Windows has no use for, and the separators that leaves
/// at either end or doubled.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for e in entries {
        match &e {
            Entry::Standard(s) if standard_label(*s).is_none() => continue,
            Entry::Separator if matches!(out.last(), None | Some(Entry::Separator)) => continue,
            _ => {}
        }
        out.push(e);
    }
    while matches!(out.last(), Some(Entry::Separator)) {
        out.pop();
    }
    out
}

/// The label of a standard item, with its shortcut where the system or
/// the focused control provides it; `None` for those Windows lacks.
fn standard_label(s: Standard) -> Option<&'static str> {
    Some(match s {
        Standard::About => "About Worktree Manager",
        Standard::Quit => "Exit\tAlt+F4",
        Standard::Undo => "Undo\tCtrl+Z",
        Standard::Cut => "Cut\tCtrl+X",
        Standard::Copy => "Copy\tCtrl+C",
        Standard::Paste => "Paste\tCtrl+V",
        Standard::SelectAll => "Select All\tCtrl+A",
        Standard::Minimize => "Minimize",
        Standard::Zoom => "Maximize",
        // An edit control has one level of undo and no redo; Close Window
        // would be Exit for the main window.
        Standard::Redo
        | Standard::CloseWindow
        | Standard::Services
        | Standard::Hide
        | Standard::HideOthers
        | Standard::ShowAll
        | Standard::BringAllToFront => return None,
    })
}

fn key_text(k: KeyName) -> String {
    match k {
        KeyName::Char(',') => ",".into(),
        KeyName::Char(c) => c.to_ascii_uppercase().to_string(),
        KeyName::Up => "Up".into(),
        KeyName::Down => "Down".into(),
    }
}

pub fn shortcut_text(s: &Shortcut) -> String {
    let mut t = String::from("Ctrl+");
    if s.shift {
        t.push_str("Shift+");
    }
    if s.alt {
        t.push_str("Alt+");
    }
    t.push_str(&key_text(s.key));
    t
}

fn virtual_key(k: KeyName) -> Option<u16> {
    Some(match k {
        KeyName::Up => VK_UP.0,
        KeyName::Down => VK_DOWN.0,
        KeyName::Char(c) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase() as u16,
        KeyName::Char(',') => VK_OEM_COMMA.0,
        KeyName::Char('.') => VK_OEM_PERIOD.0,
        KeyName::Char('-') => VK_OEM_MINUS.0,
        KeyName::Char('=') => VK_OEM_PLUS.0,
        KeyName::Char('/') => VK_OEM_2.0,
        _ => return None,
    })
}

fn accel(s: &Shortcut, cmd: u32) -> Option<ACCEL> {
    let mut flags = FVIRTKEY | FCONTROL;
    if s.shift {
        flags |= FSHIFT;
    }
    if s.alt {
        flags |= FALT;
    }
    Some(ACCEL {
        fVirt: flags,
        key: virtual_key(s.key)?,
        cmd: cmd as u16,
    })
}

/// A menu item or shortcut was chosen. The queue has been drained, so the
/// enabled state is current.
pub fn command(id: u32) {
    if id >= FIRST_ACTION {
        let h = with_state(|s| s.menus.action(id)).flatten();
        if let Some(h) = h {
            h.call(());
        }
        return;
    }
    if id >= FIRST_STANDARD {
        if let Some(s) = STANDARD.get((id - FIRST_STANDARD) as usize) {
            standard(*s);
        }
    }
}

impl Menus {
    fn action(&self, id: u32) -> Option<Handler> {
        let (h, enabled) = self.actions.get(&id)?;
        enabled.then(|| h.clone())
    }
}

fn standard(s: Standard) {
    let focus = unsafe { GetFocus() };
    let main = main_window();
    match s {
        Standard::Quit => crate::app::quit(),
        Standard::About => later(move || {
            let text = format!(
                "Worktree Manager {}\n\nGit worktrees across your repositories.",
                env!("CARGO_PKG_VERSION")
            );
            nested(main, || unsafe {
                MessageBoxW(
                    Some(main),
                    &HSTRING::from(text),
                    &HSTRING::from("About Worktree Manager"),
                    MB_OK | MB_ICONINFORMATION,
                )
            });
        }),
        Standard::Undo => {
            send(focus, WM_UNDO, 0, 0);
        }
        Standard::Cut => {
            send(focus, WM_CUT, 0, 0);
        }
        Standard::Copy => {
            send(focus, WM_COPY, 0, 0);
        }
        Standard::Paste => {
            send(focus, WM_PASTE, 0, 0);
        }
        Standard::SelectAll => {
            send(focus, EM_SETSEL, 0, -1);
        }
        Standard::Minimize => unsafe {
            let _ = ShowWindow(main, SW_MINIMIZE);
        },
        Standard::Zoom => unsafe {
            let zoomed = IsZoomed(main).as_bool();
            let _ = ShowWindow(main, if zoomed { SW_RESTORE } else { SW_MAXIMIZE });
        },
        _ => {}
    }
}
