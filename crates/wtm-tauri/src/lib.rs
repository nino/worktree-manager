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
    // Reads the config and the last run's snapshot; no git yet.
    let app = App::new(platform, dirs);
    let boot = wire::Boot {
        model: wire::Model::from_core(&app.model()),
        file_manager,
        os: std::env::consts::OS.to_string(),
        closed_repos: app.ui_state().collapsed_repos.into_iter().collect(),
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
