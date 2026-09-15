//! The facade a UI backend talks to.
//!
//! Design rules that keep the UI latency-free:
//!
//! - [`App::dispatch`] never blocks. It applies whatever can be known
//!   immediately to the model (a spinner, a placeholder row, a cleared notice),
//!   emits [`Event::ModelChanged`] so the UI repaints at once, and hands the
//!   slow part to the tokio runtime.
//! - Results arrive the same way: the model is replaced and an event fires.
//!   The UI never polls and never calls git.
//! - Listeners are invoked on whatever thread finished the work; the UI backend
//!   marshals to its main thread (a cheap `dispatch_async`) and coalesces
//!   bursts. That marshalling is the only place a toolkit's threading rules
//!   need to be known.
//! - Listing is per repo and per worktree, so a filesystem event re-reads one
//!   worktree's status, not the world. A cached snapshot of the last listing is
//!   written to disk and shown instantly at the next launch while git catches up.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use log::{info, warn};
use parking_lot::{Mutex, RwLock};
use tokio::runtime::Runtime;
use wtm_platform::{AppDirs, Platform};

use crate::command::build_command;
use crate::config::ConfigStore;
use crate::fetcher::{fetch_all, FETCH_INTERVAL};
use crate::git::{
    list_base_ref_candidates, list_branches, list_worktrees, resolve_trunk_ref, worktree_status,
};
use crate::model::{Busy, Model, Notice, PendingCreation, RepoNode, Tone};
use crate::paths::tildify;
use crate::repos::{describe_add_failure, inspect_repo};
use crate::types::*;
use crate::watcher::{linked_gitdir, WatchKey, Watcher};
use crate::worktrees;

/// Something the UI wants done.
pub enum Action {
    /// Re-list every repo (worktrees, statuses, branches).
    RefreshAll,
    /// Re-list one repo.
    RefreshRepo(String),
    /// Re-read one worktree's status only.
    RefreshWorktree {
        repo_id: String,
        path: String,
    },
    /// Add repositories by path (picker or drop). Outcome lands in the notice.
    AddRepos(Vec<PathBuf>),
    UpdateRepo(RepoConfig),
    RemoveRepo(String),
    SetSettings(AppSettings),
    /// Start a creation; a placeholder row appears until git lists the result.
    CreateWorktree(CreateWorktreeParams),
    /// Forget a failed creation's error row.
    DismissCreation(u64),
    /// Delete with the safety ladder; the reply drives the confirmation flow.
    DeleteWorktree(DeleteWorktreeParams, Reply<DeleteWorktreeResult>),
    Push {
        repo_id: String,
        path: String,
    },
    Pull {
        repo_id: String,
        path: String,
    },
    PullMain {
        repo_id: String,
        path: String,
    },
    Switch {
        repo_id: String,
        path: String,
        branch: String,
    },
    OpenInEditor(String),
    OpenInTerminal(String),
    Reveal(String),
    ClearNotice,
    /// Say something in the notice bar. Used by anything outside the core's
    /// own operations that has news — the updater, for one.
    ShowNotice {
        text: String,
        tone: Tone,
    },
}

/// Callback for actions that need an answer beyond a model change.
pub type Reply<T> = Box<dyn FnOnce(T) + Send + 'static>;

/// What listeners hear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `App::model()` has a new snapshot.
    ModelChanged,
}

type Listener = Box<dyn Fn(Event) + Send + Sync + 'static>;

struct Inner {
    rt: Runtime,
    platform: Arc<dyn Platform>,
    dirs: AppDirs,
    store: Mutex<ConfigStore>,
    model: RwLock<Arc<Model>>,
    listeners: Mutex<Vec<Listener>>,
    /// Serialises mutating operations per worktree path, so a delete can never
    /// race a push that is still running on the same worktree.
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Per-repo refresh sequence: a listing that started before a later one
    /// was applied is discarded, so results never go backwards in time.
    repo_seq: Mutex<HashMap<String, (u64, u64)>>,
    watcher: Mutex<Option<Watcher>>,
    next_id: AtomicU64,
}

/// Thread-safe handle to the application. Cheap to clone.
#[derive(Clone)]
pub struct App {
    inner: Arc<Inner>,
}

impl App {
    /// Build the app: load config and the cached snapshot (nothing touches git
    /// yet), start the runtime. Call [`App::start`] once the UI is listening.
    pub fn new(platform: Arc<dyn Platform>, dirs: AppDirs) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("wtm-core")
            .enable_all()
            .build()
            .expect("tokio runtime");
        let store = ConfigStore::load(&dirs);
        let home = dirs.home.to_string_lossy().into_owned();
        let mut model = Model {
            config: store.config().clone(),
            repos: Vec::new(),
            busy: Default::default(),
            pending: Vec::new(),
            notice: None,
            refreshing: false,
            fetching: false,
            home,
        };
        model.repos = load_snapshot(&dirs).unwrap_or_default();
        model.sync_repos_with_config();
        let inner = Inner {
            rt,
            platform,
            dirs,
            store: Mutex::new(store),
            model: RwLock::new(Arc::new(model)),
            listeners: Mutex::new(Vec::new()),
            locks: Mutex::new(HashMap::new()),
            repo_seq: Mutex::new(HashMap::new()),
            watcher: Mutex::new(None),
            next_id: AtomicU64::new(1),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Begin background work: the first full listing, the file watcher, and the
    /// periodic fetch loop.
    pub fn start(&self) {
        let app = self.clone();
        match Watcher::new(move |key: WatchKey| {
            app.dispatch(Action::RefreshWorktree {
                repo_id: key.repo_id,
                path: key.worktree_path,
            })
        }) {
            Ok(w) => *self.inner.watcher.lock() = Some(w),
            Err(e) => warn!("file watching unavailable: {e}"),
        }
        self.dispatch(Action::RefreshAll);
        let app = self.clone();
        self.inner.rt.spawn(async move {
            loop {
                app.fetch_cycle().await;
                tokio::time::sleep(FETCH_INTERVAL).await;
            }
        });
    }

    /// Register a change listener. Called from arbitrary threads.
    pub fn subscribe(&self, f: impl Fn(Event) + Send + Sync + 'static) {
        self.inner.listeners.lock().push(Box::new(f));
    }

    /// The current snapshot.
    pub fn model(&self) -> Arc<Model> {
        self.inner.model.read().clone()
    }

    /// Abbreviate a path with `~`.
    pub fn display_path(&self, path: &str) -> String {
        tildify(path, &self.model().home)
    }

    pub fn platform(&self) -> &dyn Platform {
        &*self.inner.platform
    }

    // MARK: Dispatch

    /// Apply an action. Never blocks; never fails.
    pub fn dispatch(&self, action: Action) {
        match action {
            Action::RefreshAll => self.refresh_all(),
            Action::RefreshRepo(id) => self.spawn_refresh_repo(id),
            Action::RefreshWorktree { repo_id, path } => self.spawn_refresh_worktree(repo_id, path),
            Action::AddRepos(paths) => self.add_repos(paths),
            Action::UpdateRepo(repo) => {
                let id = repo.id.clone();
                let changed_trunk = self
                    .model()
                    .repo(&id)
                    .map(|n| n.repo.main_branch != repo.main_branch)
                    .unwrap_or(true);
                self.inner.store.lock().update_repo(repo);
                self.apply_config();
                if changed_trunk {
                    self.spawn_refresh_repo(id);
                }
            }
            Action::RemoveRepo(id) => {
                self.inner.store.lock().remove_repo(&id);
                self.apply_config();
                self.rewatch();
            }
            Action::SetSettings(s) => {
                self.inner.store.lock().set_settings(s);
                self.apply_config();
            }
            Action::CreateWorktree(params) => self.create_worktree(params),
            Action::DismissCreation(id) => self.update(|m| m.pending.retain(|p| p.id != id)),
            Action::DeleteWorktree(params, reply) => self.delete_worktree(params, reply),
            Action::Push { repo_id, path } => self.git_op(repo_id, path, Busy::Pushing, |r, p| {
                Box::pin(worktrees::push(r, p))
            }),
            Action::Pull { repo_id, path } => self.git_op(repo_id, path, Busy::Pulling, |r, p| {
                Box::pin(worktrees::pull(r, p))
            }),
            Action::PullMain { repo_id, path } => {
                self.git_op(repo_id, path, Busy::Merging, |r, p| {
                    Box::pin(worktrees::pull_main(r, p))
                })
            }
            Action::Switch {
                repo_id,
                path,
                branch,
            } => self.git_op(repo_id, path, Busy::Switching, move |r, p| {
                let branch = branch.clone();
                Box::pin(async move { worktrees::switch_branch(r, p, &branch).await })
            }),
            Action::OpenInEditor(path) => {
                let cmd = self.model().config.editor_command.trim().to_string();
                let cmd = if cmd.is_empty() {
                    "code".to_string()
                } else {
                    cmd
                };
                if let Err(e) = self
                    .inner
                    .platform
                    .spawn_detached(&build_command(&cmd, &path))
                {
                    self.notify(Tone::Error, format!("Could not open editor: {e}"));
                }
            }
            Action::OpenInTerminal(path) => {
                if let Err(e) = self
                    .inner
                    .platform
                    .open_in_terminal(std::path::Path::new(&path))
                {
                    self.notify(Tone::Error, format!("Could not open terminal: {e}"));
                }
            }
            Action::Reveal(path) => {
                if let Err(e) = self.inner.platform.reveal(std::path::Path::new(&path)) {
                    self.notify(Tone::Error, format!("Could not reveal folder: {e}"));
                }
            }
            Action::ClearNotice => self.update(|m| m.notice = None),
            Action::ShowNotice { text, tone } => self.notify(tone, text),
        }
    }

    // MARK: Model plumbing

    /// Mutate a copy of the model, publish it, notify listeners.
    fn update(&self, f: impl FnOnce(&mut Model)) {
        {
            let mut guard = self.inner.model.write();
            let mut next = (**guard).clone();
            f(&mut next);
            if next == **guard {
                return;
            }
            *guard = Arc::new(next);
        }
        self.emit(Event::ModelChanged);
    }

    fn emit(&self, event: Event) {
        for l in self.inner.listeners.lock().iter() {
            l(event);
        }
    }

    fn notify(&self, tone: Tone, text: String) {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.update(|m| m.notice = Some(Notice { tone, text, id }));
    }

    fn apply_config(&self) {
        let config = self.inner.store.lock().config().clone();
        self.update(|m| {
            m.config = config;
            m.sync_repos_with_config();
        });
    }

    fn repo_config(&self, repo_id: &str) -> Option<RepoConfig> {
        self.inner.store.lock().repo(repo_id).cloned()
    }

    fn lock_for(&self, path: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.inner
            .locks
            .lock()
            .entry(path.to_string())
            .or_default()
            .clone()
    }

    fn save_snapshot(&self) {
        let repos = self.model().repos.clone();
        let path = self.inner.dirs.config_dir.join("snapshot.json");
        self.inner.rt.spawn(async move {
            if let Ok(json) = serde_json::to_vec(&repos) {
                let tmp = path.with_extension("json.tmp");
                if tokio::fs::write(&tmp, json).await.is_ok() {
                    let _ = tokio::fs::rename(&tmp, &path).await;
                }
            }
        });
    }

    // MARK: Listing

    fn refresh_all(&self) {
        let ids: Vec<String> = self
            .model()
            .repos
            .iter()
            .map(|r| r.repo.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        self.update(|m| m.refreshing = true);
        let app = self.clone();
        self.inner.rt.spawn(async move {
            let started = std::time::Instant::now();
            let count = ids.len();
            let mut set = tokio::task::JoinSet::new();
            for id in ids {
                let app = app.clone();
                set.spawn(async move { app.refresh_repo(id).await });
            }
            while set.join_next().await.is_some() {}
            app.update(|m| m.refreshing = false);
            app.rewatch();
            info!(
                "refreshed {count} repo(s) in {} ms",
                started.elapsed().as_millis()
            );
        });
    }

    fn spawn_refresh_repo(&self, id: String) {
        let app = self.clone();
        self.inner.rt.spawn(async move {
            app.refresh_repo(id).await;
            app.rewatch();
        });
    }

    async fn refresh_repo(&self, repo_id: String) {
        let Some(repo) = self.repo_config(&repo_id) else {
            return;
        };
        let seq = {
            let mut m = self.inner.repo_seq.lock();
            let e = m.entry(repo_id.clone()).or_insert((0, 0));
            e.0 += 1;
            e.0
        };
        let trunk = resolve_trunk_ref(&repo.path, &repo.main_branch).await;
        let (worktrees, branches, candidates) = tokio::join!(
            list_worktrees(&repo.path, &trunk),
            list_branches(&repo.path),
            list_base_ref_candidates(&repo.path)
        );
        {
            let mut m = self.inner.repo_seq.lock();
            let e = m.entry(repo_id.clone()).or_insert((0, 0));
            if seq < e.1 {
                return; // a newer listing already landed
            }
            e.1 = seq;
        }
        let (worktrees, error) = match worktrees {
            Ok(w) => (w, None),
            Err(e) => (Vec::new(), Some(e.detail())),
        };
        let branches = branches.unwrap_or_default();
        let candidates = candidates.unwrap_or_default();
        self.update(|m| {
            // A creation whose branch git now lists has landed.
            m.pending.retain(|p| {
                p.repo_id != repo_id
                    || p.error.is_some()
                    || !worktrees
                        .iter()
                        .any(|w| w.branch.as_deref() == Some(&p.branch))
            });
            if let Some(node) = m.repo_mut(&repo_id) {
                node.worktrees = worktrees;
                node.default_base_ref = trunk;
                node.branches = branches;
                node.base_ref_candidates = candidates;
                node.error = error;
                node.loaded = true;
            }
        });
        self.save_snapshot();
    }

    fn spawn_refresh_worktree(&self, repo_id: String, path: String) {
        let app = self.clone();
        self.inner
            .rt
            .spawn(async move { app.refresh_worktree(&repo_id, &path).await });
    }

    async fn refresh_worktree(&self, repo_id: &str, path: &str) {
        let started = std::time::Instant::now();
        let Some(node) = self.model().repo(repo_id).cloned() else {
            return;
        };
        if node.worktree(path).map(|w| w.prunable).unwrap_or(true) {
            // Unknown or missing on disk: only a full listing can say more.
            self.refresh_repo(repo_id.to_string()).await;
            return;
        }
        let status = match worktree_status(path, &node.default_base_ref).await {
            Ok(s) => Some(s),
            Err(_) => {
                // The folder may have vanished; only a listing can tell
                // "status unavailable" from "prunable".
                self.refresh_repo(repo_id.to_string()).await;
                return;
            }
        };
        // The branch may have changed under us (git switch in a terminal).
        let branch = crate::git::run_git(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_string());
        let head = crate::git::run_git(path, &["rev-parse", "--short=12", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_string());
        let repo_id = repo_id.to_string();
        let path = path.to_string();
        self.update(|m| {
            if let Some(w) = m
                .repo_mut(&repo_id)
                .and_then(|n| n.worktrees.iter_mut().find(|w| w.path == path))
            {
                w.status = status;
                w.branch = branch;
                if let Some(h) = head {
                    w.head = h;
                }
            }
        });
        self.save_snapshot();
        log::debug!("refreshed {path} in {} ms", started.elapsed().as_millis());
    }

    /// Point the file watcher at the current worktree set.
    fn rewatch(&self) {
        let model = self.model();
        let mut entries = Vec::new();
        for node in &model.repos {
            for w in &node.worktrees {
                if w.prunable {
                    continue;
                }
                let dir = PathBuf::from(&w.path);
                let extras = linked_gitdir(&dir).into_iter().collect();
                entries.push((
                    dir,
                    extras,
                    WatchKey {
                        repo_id: node.repo.id.clone(),
                        worktree_path: w.path.clone(),
                    },
                ));
            }
        }
        if let Some(w) = self.inner.watcher.lock().as_mut() {
            w.set_watched(entries);
        }
    }

    // MARK: Repos

    fn add_repos(&self, paths: Vec<PathBuf>) {
        let app = self.clone();
        self.update(|m| m.notice = None);
        self.inner.rt.spawn(async move {
            let mut result = AddReposResult::default();
            let mut new_ids = Vec::new();
            // Sequential on purpose: duplicates within one batch must be caught.
            for path in paths {
                match inspect_repo(&path).await {
                    Ok((root, main)) => {
                        let added = app.inner.store.lock().add_repo(root.clone(), main);
                        match added {
                            Some(name) => {
                                result.added.push(name);
                                if let Some(r) = app
                                    .inner
                                    .store
                                    .lock()
                                    .config()
                                    .repos
                                    .iter()
                                    .find(|r| r.path == root)
                                {
                                    new_ids.push(r.id.clone());
                                }
                            }
                            None => result.failed.push(AddRepoFailure {
                                path: path.to_string_lossy().into_owned(),
                                message: "Already added".into(),
                            }),
                        }
                    }
                    Err(e) => result.failed.push(AddRepoFailure {
                        path: path.to_string_lossy().into_owned(),
                        message: describe_add_failure(&e),
                    }),
                }
            }
            app.apply_config();
            if let Some((tone, text)) = summarize_add_result(&result, &app.model().home) {
                app.notify(tone, text);
            }
            let mut set = tokio::task::JoinSet::new();
            for id in new_ids {
                let app = app.clone();
                set.spawn(async move { app.refresh_repo(id).await });
            }
            while set.join_next().await.is_some() {}
            app.rewatch();
        });
    }

    // MARK: Worktrees

    fn create_worktree(&self, params: CreateWorktreeParams) {
        let Some(repo) = self.repo_config(&params.repo_id) else {
            return;
        };
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let pending = PendingCreation {
            id,
            repo_id: repo.id.clone(),
            branch: params.branch.trim().to_string(),
            error: None,
        };
        self.update(|m| m.pending.push(pending));
        let app = self.clone();
        self.inner.rt.spawn(async move {
            let root = app.model().config.worktrees_root.clone();
            let target = crate::paths::worktree_path_for(&root, &repo.name, params.branch.trim());
            let lock = app.lock_for(&target.to_string_lossy());
            let outcome = {
                let _g = lock.lock().await;
                worktrees::create_worktree(&repo, &root, &params).await
            };
            match outcome {
                Ok(path) => {
                    let init = repo.init_command.trim().to_string();
                    if !init.is_empty() {
                        // Fire and forget in the worktree, through the login
                        // shell so the user's PATH applies.
                        let line = format!("cd {} && {init}", crate::command::shell_quote(&path));
                        if let Err(e) = app.inner.platform.spawn_detached(&line) {
                            warn!("init command failed to start: {e}");
                        }
                    }
                    app.refresh_repo(repo.id.clone()).await;
                    app.update(|m| m.pending.retain(|p| p.id != id));
                    app.rewatch();
                }
                Err(message) => app.update(|m| {
                    if let Some(p) = m.pending.iter_mut().find(|p| p.id == id) {
                        p.error = Some(message);
                    }
                }),
            }
        });
    }

    fn delete_worktree(&self, params: DeleteWorktreeParams, reply: Reply<DeleteWorktreeResult>) {
        let Some(repo) = self.repo_config(&params.repo_id) else {
            reply(DeleteWorktreeResult::refused(
                DeleteRefusal::Error,
                "Unknown repository.",
            ));
            return;
        };
        let path = params.worktree_path.clone();
        self.update(|m| {
            m.busy.insert(path.clone(), Busy::Deleting);
        });
        let app = self.clone();
        self.inner.rt.spawn(async move {
            let lock = app.lock_for(&path);
            let result = {
                let _g = lock.lock().await;
                worktrees::delete_worktree(&repo, &params).await
            };
            if result.ok {
                app.refresh_repo(repo.id.clone()).await;
                app.rewatch();
            }
            app.update(|m| {
                m.busy.remove(&path);
            });
            reply(result);
        });
    }

    fn git_op<F>(&self, repo_id: String, path: String, busy: Busy, op: F)
    where
        F: for<'a> FnOnce(
                &'a RepoConfig,
                &'a str,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = GitOpResult> + Send + 'a>,
            > + Send
            + 'static,
    {
        let Some(repo) = self.repo_config(&repo_id) else {
            return;
        };
        self.update(|m| {
            m.busy.insert(path.clone(), busy);
            m.notice = None;
        });
        let app = self.clone();
        self.inner.rt.spawn(async move {
            let lock = app.lock_for(&path);
            let result = {
                let _g = lock.lock().await;
                op(&repo, &path).await
            };
            app.refresh_worktree(&repo_id, &path).await;
            app.update(|m| {
                m.busy.remove(&path);
            });
            if !result.ok {
                let label = busy.label().trim_end_matches('…');
                app.notify(Tone::Error, format!("{label} failed: {}", result.message));
            }
        });
    }

    // MARK: Background fetch

    async fn fetch_cycle(&self) {
        let repos = self.model().config.repos.clone();
        if repos.is_empty() {
            return;
        }
        self.update(|m| m.fetching = true);
        let fetched = fetch_all(repos).await;
        self.update(|m| m.fetching = false);
        if fetched > 0 {
            info!("auto-fetch: {fetched} repo(s) fetched");
            self.refresh_all();
        }
    }
}

fn load_snapshot(dirs: &AppDirs) -> Option<Vec<RepoNode>> {
    let text = std::fs::read_to_string(dirs.config_dir.join("snapshot.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// One line describing the outcome of adding repositories. Any rejected path
/// makes it an error notice.
pub fn summarize_add_result(result: &AddReposResult, home: &str) -> Option<(Tone, String)> {
    let mut parts = Vec::new();
    if !result.added.is_empty() {
        parts.push(format!("Added {}", result.added.join(", ")));
    }
    for f in &result.failed {
        parts.push(format!("{}: {}", tildify(&f.path, home), f.message));
    }
    if parts.is_empty() {
        return None;
    }
    let tone = if result.failed.is_empty() {
        Tone::Info
    } else {
        Tone::Error
    };
    Some((tone, parts.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarises_adds_and_failures() {
        let r = AddReposResult {
            added: vec!["a".into(), "b".into()],
            failed: vec![AddRepoFailure {
                path: "/Users/me/x".into(),
                message: "Not a git repository".into(),
            }],
        };
        let (tone, text) = summarize_add_result(&r, "/Users/me").unwrap();
        assert_eq!(tone, Tone::Error);
        assert_eq!(text, "Added a, b · ~/x: Not a git repository");
        assert!(summarize_add_result(&AddReposResult::default(), "").is_none());
    }
}
