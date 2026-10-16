//! The Tauri shell around the core: one [`wtm_core::App`], its model pushed
//! to the webview as JSON on every change, and its actions exposed as
//! commands. Nothing here decides what the window shows; `ui/` does.

pub mod frame;
mod menu;
pub mod wire;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use log::{info, warn};
use tauri::{
    AppHandle, DragDropEvent, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder,
    Window, WindowEvent,
};
use wtm_core::{Action, App};

use frame::{Origin, Rect};

/// Shared with every command.
struct Bridge {
    app: App,
    started: Instant,
    shown: AtomicBool,
    /// The menu items that follow the selection; macOS only.
    menu: OnceLock<menu::Items>,
}

impl Bridge {
    /// Show the window, once. The page asks when it has painted; a timer
    /// asks too, so that a page that fails to load still shows something.
    fn show(&self, handle: &AppHandle, why: &str) {
        if self.shown.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
        info!(
            "window shown ({why}) {:.0?} after launch",
            self.started.elapsed()
        );
    }
}

/// How long the window may stay hidden waiting for the page's first paint.
const SHOW_ANYWAY_AFTER: Duration = Duration::from_millis(1500);

pub fn run() {
    let started = Instant::now();
    let (platform, dirs) = wtm_os::native();
    let file_manager = platform.file_manager_name().to_string();
    let terminal_note = platform.terminal_note().to_string();
    // Reads the config and the last run's snapshot; no git yet.
    let app = App::new(platform, dirs);
    let boot = wire::Boot {
        model: wire::Model::from_core(&app.model()),
        file_manager,
        os: std::env::consts::OS.to_string(),
        terminal_note,
        view: wire::ViewState::from_core(&app.ui_state()),
        menu_bar: MENU_BAR,
    };
    let script = format!(
        "window.__WTM_BOOT__ = {};",
        serde_json::to_string(&boot).expect("boot JSON")
    );
    let bridge = Bridge {
        app: app.clone(),
        started,
        shown: AtomicBool::new(false),
        menu: OnceLock::new(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(bridge)
        .invoke_handler(tauri::generate_handler![
            ready,
            refresh,
            refresh_repo,
            clear_notice,
            open_in_editor,
            open_in_terminal,
            reveal,
            push,
            pull,
            pull_main,
            switch_branch,
            delete_worktree,
            store_view,
            set_menu_state,
            check_new_worktree,
            create_worktree,
            fetch_repo,
            dismiss_creation,
            add_repos,
            pick_folder,
            update_repo,
            remove_repo,
            move_repo,
            set_settings,
        ])
        .on_menu_event(|handle, event| {
            let id = event.id().as_ref();
            if menu::IDS.contains(&id) {
                let _ = handle.emit("menu", id);
            }
        })
        .setup(move |tauri_app| {
            if MENU_BAR {
                let (bar, items) = menu::build(tauri_app.handle())?;
                tauri_app.set_menu(bar)?;
                let _ = tauri_app.state::<Bridge>().menu.set(items);
            }
            let window = WebviewWindowBuilder::new(tauri_app, "main", WebviewUrl::default())
                .title("Worktree Manager")
                .min_inner_size(520.0, 320.0)
                .visible(false)
                .initialization_script(&script);
            // Back where it was, unless that is no longer on a screen.
            let to_rect = |m: &tauri::Monitor| {
                let area = m.work_area();
                let s = m.scale_factor();
                Rect {
                    x: area.position.x as f64 / s,
                    y: area.position.y as f64 / s,
                    width: area.size.width as f64 / s,
                    height: area.size.height as f64 / s,
                }
            };
            let screens: Vec<Rect> = tauri_app
                .available_monitors()
                .unwrap_or_default()
                .iter()
                .map(to_rect)
                .collect();
            let main_screen = tauri_app
                .primary_monitor()
                .ok()
                .flatten()
                .map(|m| to_rect(&m));
            let window =
                match frame::restore(app.ui_state().window, origin(tauri_app.handle()), &screens) {
                    Some(r) => window.position(r.x, r.y).inner_size(r.width, r.height),
                    None => {
                        let (width, height) = frame::first_size(main_screen);
                        window.inner_size(width, height).center()
                    }
                };
            // As the AppKit build's unified toolbar: the page's toolbar runs
            // under the traffic lights, which sit centred in its 52 points.
            // tao only grows the title bar to the button's height + y and
            // leaves each button where it sat, about 16 pt above the bar's
            // bottom edge, so y = 42 − 15 puts their centres at 26 pt.
            #[cfg(target_os = "macos")]
            let window = window
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(20.0, 27.0));
            let window = window.build()?;
            window.on_window_event({
                let app = app.clone();
                let handle = tauri_app.handle().clone();
                let window = window.as_ref().window();
                move |event| match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                        save_frame(&window, &app, &handle);
                    }
                    WindowEvent::DragDrop(drop) => dropped(drop, &app, &handle),
                    _ => {}
                }
            });
            let handle = tauri_app.handle().clone();
            forward_models(&app, handle.clone());
            app.start();
            std::thread::spawn(move || {
                std::thread::sleep(SHOW_ANYWAY_AFTER);
                handle.state::<Bridge>().show(&handle, "timeout");
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the app")
        .run(|handle, event| match event {
            // The delayed write of the window's state never lands once
            // the process is gone.
            RunEvent::Exit => handle.state::<Bridge>().app.flush_ui_state(),
            // Folders dropped on the Dock icon, or opened with the app.
            #[cfg(target_os = "macos")]
            RunEvent::Opened { urls } => {
                let paths: Vec<PathBuf> =
                    urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
                add_folders(paths, &handle.state::<Bridge>().app);
            }
            _ => {}
        });
}

/// macOS has a menu bar with the app's shortcuts (`menu.rs`).
const MENU_BAR: bool = cfg!(target_os = "macos");

// MARK: Window

/// How this OS measures a saved frame (`frame.rs`).
fn origin(handle: &AppHandle) -> Origin {
    if !cfg!(target_os = "macos") {
        return Origin::Top;
    }
    match handle.primary_monitor() {
        Ok(Some(m)) => Origin::Bottom {
            main_height: m.size().height as f64 / m.scale_factor(),
        },
        _ => Origin::Top,
    }
}

/// Record where the window is, for the next launch. A minimised, maximised
/// or full-screen window keeps the frame it had before.
fn save_frame(window: &Window, app: &App, handle: &AppHandle) {
    let normal = !window.is_minimized().unwrap_or(true)
        && !window.is_maximized().unwrap_or(true)
        && !window.is_fullscreen().unwrap_or(true);
    let (Ok(scale), Ok(pos), Ok(size)) = (
        window.scale_factor(),
        window.outer_position(),
        window.inner_size(),
    ) else {
        return;
    };
    if !normal || !window.is_visible().unwrap_or(false) {
        return;
    }
    let pos = pos.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let rect = Rect {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    };
    let mut state = app.ui_state();
    state.window = Some(origin(handle).save(rect));
    app.store_ui_state(state);
}

/// Folders dragged over the window: the page outlines the list while they
/// are over it, and a drop adds each as a repo.
fn dropped(event: &DragDropEvent, app: &App, handle: &AppHandle) {
    match event {
        DragDropEvent::Enter { paths, .. } => {
            let _ = handle.emit("files-over", paths.iter().any(|p| p.is_dir()));
        }
        DragDropEvent::Drop { paths, .. } => {
            let _ = handle.emit("files-over", false);
            add_folders(paths.clone(), app);
        }
        DragDropEvent::Leave => {
            let _ = handle.emit("files-over", false);
        }
        _ => {}
    }
}

/// Add each folder as a repo; how that went lands in the notice bar. A
/// file is not a repo and is passed over without a word.
fn add_folders(paths: Vec<PathBuf>, app: &App) {
    let folders: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_dir()).collect();
    if !folders.is_empty() {
        app.dispatch(Action::AddRepos(folders));
    }
}

/// Push every new model to the page. The core calls listeners on whichever
/// thread finished; they only wake this one thread, and a wake that arrives
/// while one is queued is dropped, so a burst of changes is one event
/// carrying the latest model.
fn forward_models(app: &App, handle: AppHandle) {
    let (wake, woken) = sync_channel::<()>(1);
    app.subscribe(move |_| {
        let _ = wake.try_send(());
    });
    let app = app.clone();
    std::thread::Builder::new()
        .name("wtm-emit".into())
        .spawn(move || {
            let mut last: Option<wire::Model> = None;
            for () in woken {
                let model = wire::Model::from_core(&app.model());
                // The core's model holds more than the page sees (remote
                // branches, the config); a change only there is no news.
                if last.as_ref() == Some(&model) {
                    continue;
                }
                if let Err(e) = handle.emit("model", &model) {
                    warn!("could not send the model to the page: {e}");
                }
                last = Some(model);
            }
        })
        .expect("emitter thread");
}

// MARK: Commands

/// The page has painted its first frame. Returns the model as it is now,
/// which may have moved on since the boot snapshot was taken.
#[tauri::command]
fn ready(handle: AppHandle, bridge: State<'_, Bridge>) -> wire::Model {
    bridge.show(&handle, "first paint");
    wire::Model::from_core(&bridge.app.model())
}

#[tauri::command]
fn refresh(bridge: State<'_, Bridge>) {
    bridge.app.dispatch(Action::RefreshAll);
}

#[tauri::command]
fn refresh_repo(bridge: State<'_, Bridge>, repo_id: String) {
    bridge.app.dispatch(Action::RefreshRepo(repo_id));
}

#[tauri::command]
fn clear_notice(bridge: State<'_, Bridge>) {
    bridge.app.dispatch(Action::ClearNotice);
}

#[tauri::command]
fn open_in_editor(bridge: State<'_, Bridge>, path: String) {
    bridge.app.dispatch(Action::OpenInEditor(path));
}

#[tauri::command]
fn open_in_terminal(bridge: State<'_, Bridge>, path: String) {
    bridge.app.dispatch(Action::OpenInTerminal(path));
}

#[tauri::command]
fn reveal(bridge: State<'_, Bridge>, path: String) {
    bridge.app.dispatch(Action::Reveal(path));
}

#[tauri::command]
fn push(bridge: State<'_, Bridge>, repo_id: String, path: String) {
    bridge.app.dispatch(Action::Push { repo_id, path });
}

#[tauri::command]
fn pull(bridge: State<'_, Bridge>, repo_id: String, path: String) {
    bridge.app.dispatch(Action::Pull { repo_id, path });
}

#[tauri::command]
fn pull_main(bridge: State<'_, Bridge>, repo_id: String, path: String) {
    bridge.app.dispatch(Action::PullMain { repo_id, path });
}

#[tauri::command]
fn switch_branch(bridge: State<'_, Bridge>, repo_id: String, path: String, branch: String) {
    bridge.app.dispatch(Action::Switch {
        repo_id,
        path,
        branch,
    });
}

/// Runs the core's safety ladder and answers how it ended; a refusal is
/// the page's cue to ask the next question.
#[tauri::command]
async fn delete_worktree(
    bridge: State<'_, Bridge>,
    request: wire::DeleteRequest,
) -> Result<wire::DeleteResult, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    bridge.app.dispatch(Action::DeleteWorktree(
        request.to_core(),
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    ));
    rx.await
        .map(wire::DeleteResult::from_core)
        .map_err(|_| "the delete was dropped before it finished".to_string())
}

/// The list's closed cards, scroll and selection, for the next launch.
/// Sent on every change; an unchanged state is not written.
#[tauri::command]
fn store_view(bridge: State<'_, Bridge>, view: wire::ViewState) {
    let state = view.apply(bridge.app.ui_state());
    bridge.app.store_ui_state(state);
}

/// Enable the menu items that apply to the selection.
#[tauri::command]
fn set_menu_state(bridge: State<'_, Bridge>, state: wire::MenuState) {
    if let Some(items) = bridge.menu.get() {
        items.set(state);
    }
}

// MARK: New Worktree

/// How long the sheet waits to learn whether something is at a folder. A
/// hung network volume must not hold the sheet's check; past this the
/// folder is taken to be free, and git says otherwise on Create.
const FOLDER_LOOK_LIMIT: Duration = Duration::from_millis(400);

/// The core's verdict on the sheet's fields, against the latest listing.
/// The page asks on every keystroke and every new model.
#[tauri::command]
async fn check_new_worktree(
    bridge: State<'_, Bridge>,
    fields: wire::NewWorktreeFields,
) -> Result<wire::NewWorktreeCheck, String> {
    let model = bridge.app.model();
    let target = model
        .repo(&fields.repo_id)
        .filter(|_| !fields.name.trim().is_empty())
        .map(|n| model.worktree_path(&n.repo, &fields.name));
    let taken = match target {
        Some(path) => {
            let look = tokio::task::spawn_blocking({
                let path = path.clone();
                move || path.symlink_metadata().is_ok()
            });
            match tokio::time::timeout(FOLDER_LOOK_LIMIT, look).await {
                Ok(Ok(true)) => Some(wtm_core::paths::tildify(
                    &path.to_string_lossy(),
                    &model.home,
                )),
                _ => None,
            }
        }
        None => None,
    };
    Ok(wire::NewWorktreeCheck::new(
        &model,
        &fields,
        taken.as_deref(),
    ))
}

/// Create as the sheet's last check said; a placeholder row shows at once.
#[tauri::command]
fn create_worktree(bridge: State<'_, Bridge>, request: wire::CreateRequest) {
    bridge
        .app
        .dispatch(Action::CreateWorktree(request.to_core()));
}

/// The sheet opened: fetch now, so a branch pushed since the last cycle is
/// found by name rather than taken for a new one.
#[tauri::command]
fn fetch_repo(bridge: State<'_, Bridge>, repo_id: String) {
    bridge.app.dispatch(Action::FetchRepo(repo_id));
}

#[tauri::command]
fn dismiss_creation(bridge: State<'_, Bridge>, id: u64) {
    bridge.app.dispatch(Action::DismissCreation(id));
}

// MARK: Repos and settings

/// Ask for folders and add each as a repo; how that went lands in the
/// notice bar.
#[tauri::command]
fn add_repos(handle: AppHandle) {
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = handle.dialog().file().set_title("Add Repository");
    if let Some(w) = handle.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    let app = handle.state::<Bridge>().app.clone();
    dialog.pick_folders(move |picked| {
        let paths: Vec<_> = picked
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .collect();
        if !paths.is_empty() {
            app.dispatch(Action::AddRepos(paths));
        }
    });
}

/// One folder, for the worktrees root; `None` when cancelled.
#[tauri::command]
async fn pick_folder(handle: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = handle.dialog().file();
    if let Some(w) = handle.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
    });
    Ok(rx
        .await
        .ok()
        .flatten()
        .map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
fn update_repo(bridge: State<'_, Bridge>, edit: wire::RepoEdit) {
    let model = bridge.app.model();
    if let Some(node) = model.repo(&edit.repo_id) {
        bridge
            .app
            .dispatch(Action::UpdateRepo(edit.apply(&node.repo)));
    }
}

#[tauri::command]
fn remove_repo(bridge: State<'_, Bridge>, repo_id: String) {
    bridge.app.dispatch(Action::RemoveRepo(repo_id));
}

/// Drop a repo in the gap before `shown[gap]`, `shown` being the repos the
/// page shows (fewer than all of them during a search). Where that puts it
/// among all of them is `wtm_core::repos::drop_target`'s to say.
#[tauri::command]
fn move_repo(bridge: State<'_, Bridge>, repo_id: String, shown: Vec<String>, gap: usize) {
    let model = bridge.app.model();
    let all: Vec<&str> = model.repos.iter().map(|r| r.repo.id.as_str()).collect();
    let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
    if let Some(before) = wtm_core::repos::drop_target(&all, &shown, &repo_id, gap) {
        bridge.app.dispatch(Action::MoveRepo {
            repo_id,
            before: before.map(str::to_string),
        });
    }
}

/// Save the Settings sheet; an empty worktrees root is never saved.
#[tauri::command]
fn set_settings(bridge: State<'_, Bridge>, settings: wire::Settings) {
    let settings = settings.to_core();
    if !settings.worktrees_root.is_empty() {
        bridge.app.dispatch(Action::SetSettings(settings));
    }
}
