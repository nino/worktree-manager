//! The Tauri shell around the core: one [`wtm_core::App`], its model pushed
//! to the webview as JSON on every change, and its actions exposed as
//! commands. Nothing here decides what the window shows; `ui/` does.

pub mod wire;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use log::{info, warn};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use wtm_core::{Action, App, UiState};

/// Shared with every command.
struct Bridge {
    app: App,
    started: Instant,
    shown: AtomicBool,
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
        closed_repos: app.ui_state().collapsed_repos.into_iter().collect(),
        terminal_note,
    };
    let script = format!(
        "window.__WTM_BOOT__ = {};",
        serde_json::to_string(&boot).expect("boot JSON")
    );
    let bridge = Bridge {
        app: app.clone(),
        started,
        shown: AtomicBool::new(false),
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
            set_closed_repos,
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
        .setup(move |tauri_app| {
            let window = WebviewWindowBuilder::new(tauri_app, "main", WebviewUrl::default())
                .title("Worktree Manager")
                .inner_size(940.0, 760.0)
                .min_inner_size(520.0, 320.0)
                .visible(false)
                .initialization_script(&script);
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
            window.build()?;
            let handle = tauri_app.handle().clone();
            forward_models(&app, handle.clone());
            app.start();
            std::thread::spawn(move || {
                std::thread::sleep(SHOW_ANYWAY_AFTER);
                handle.state::<Bridge>().show(&handle, "timeout");
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the app");
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

/// Remember which cards are closed, for the next launch. The rest of the
/// window's state is kept as it was until the page reports it too.
#[tauri::command]
fn set_closed_repos(bridge: State<'_, Bridge>, ids: Vec<String>) {
    let state = bridge.app.ui_state();
    bridge.app.store_ui_state(UiState {
        collapsed_repos: ids.into_iter().collect(),
        ..state
    });
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
