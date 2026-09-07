//! Filesystem watching so a worktree's status refreshes the moment something
//! changes on disk, instead of the user pressing Refresh.
//!
//! Each worktree directory is watched recursively (FSEvents on macOS, inotify
//! on Linux, ReadDirectoryChanges on Windows — `notify` hides the difference).
//! A linked worktree's git metadata lives under the primary repo's `.git/
//! worktrees/<name>/`, so that directory is mapped to the linked worktree too:
//! a `git switch` run from a terminal updates the right row. Events are
//! debounced per worktree so a build writing thousands of files becomes one
//! refresh, not thousands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use log::{debug, warn};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

/// Quiet period before a changed worktree is refreshed.
const DEBOUNCE: Duration = Duration::from_millis(350);

/// Identifies which worktree a changed path belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WatchKey {
    pub repo_id: String,
    pub worktree_path: String,
}

/// Owns the OS watcher and the debounce thread.
pub struct Watcher {
    inner: RecommendedWatcher,
    /// Directory prefix → worktree, longest prefix wins.
    prefixes: std::sync::Arc<parking_lot::RwLock<Vec<(PathBuf, WatchKey)>>>,
    watched: Vec<PathBuf>,
}

impl Watcher {
    /// Start watching. `on_change` runs on the watcher thread for each debounced
    /// worktree; it must be cheap (dispatch to the app, return).
    pub fn new(on_change: impl Fn(WatchKey) + Send + 'static) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let inner = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )?;
        let prefixes =
            std::sync::Arc::new(parking_lot::RwLock::new(Vec::<(PathBuf, WatchKey)>::new()));
        let prefixes_for_thread = prefixes.clone();
        std::thread::Builder::new()
            .name("wtm-fs-debounce".into())
            .spawn(move || debounce_loop(rx, prefixes_for_thread, on_change))
            .expect("spawn watcher thread");
        Ok(Self {
            inner,
            prefixes,
            watched: Vec::new(),
        })
    }

    /// Replace the watched set. `entries` are `(worktree dir, extra dirs such as
    /// its gitdir, key)`.
    pub fn set_watched(&mut self, entries: Vec<(PathBuf, Vec<PathBuf>, WatchKey)>) {
        let mut prefixes = Vec::new();
        let mut want: Vec<PathBuf> = Vec::new();
        for (dir, extras, key) in entries {
            prefixes.push((dir.clone(), key.clone()));
            want.push(dir);
            for extra in extras {
                prefixes.push((extra.clone(), key.clone()));
                want.push(extra);
            }
        }
        // Longest prefix first so `.git/worktrees/x` beats the repo root.
        prefixes.sort_by_key(|(p, _)| std::cmp::Reverse(p.as_os_str().len()));
        *self.prefixes.write() = prefixes;

        for old in self.watched.iter().filter(|p| !want.contains(p)) {
            let _ = self.inner.unwatch(old);
        }
        for new in want.iter().filter(|p| !self.watched.contains(p)) {
            if let Err(e) = self.inner.watch(new, RecursiveMode::Recursive) {
                warn!("cannot watch {}: {e}", new.display());
            }
        }
        self.watched = want;
    }
}

/// Paths whose churn says nothing about status: object storage, reflogs and
/// lock files, under either a repo's `.git/` or a linked worktree's
/// `.git/worktrees/<name>/`.
fn is_noise(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.ends_with(".lock") {
        return true;
    }
    match s.find("/.git/") {
        Some(i) => {
            let inside = &s[i + "/.git/".len()..];
            inside.starts_with("objects/")
                || inside.starts_with("logs/")
                || inside.contains("/objects/")
                || inside.contains("/logs/")
        }
        None => false,
    }
}

fn debounce_loop(
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    prefixes: std::sync::Arc<parking_lot::RwLock<Vec<(PathBuf, WatchKey)>>>,
    on_change: impl Fn(WatchKey),
) {
    let mut due: HashMap<WatchKey, Instant> = HashMap::new();
    loop {
        let timeout = due
            .values()
            .min()
            .map(|t| t.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(timeout) {
            Ok(Ok(event)) => {
                let prefixes = prefixes.read();
                for path in &event.paths {
                    if is_noise(path) {
                        continue;
                    }
                    if let Some((_, key)) = prefixes.iter().find(|(p, _)| path.starts_with(p)) {
                        due.insert(key.clone(), Instant::now() + DEBOUNCE);
                    }
                }
            }
            Ok(Err(e)) => debug!("watch error: {e}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let ready: Vec<WatchKey> = due
            .iter()
            .filter(|(_, t)| **t <= now)
            .map(|(k, _)| k.clone())
            .collect();
        for key in ready {
            due.remove(&key);
            on_change(key);
        }
    }
}

/// The directory git keeps a linked worktree's metadata in (`gitdir:` line of
/// its `.git` file), if this is a linked worktree.
pub fn linked_gitdir(worktree_path: &Path) -> Option<PathBuf> {
    let dot_git = worktree_path.join(".git");
    let meta = std::fs::metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return None;
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let rel = text.trim().strip_prefix("gitdir:")?.trim();
    let p = PathBuf::from(rel);
    Some(if p.is_absolute() {
        p
    } else {
        worktree_path.join(p)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_filter_covers_repo_and_linked_gitdirs() {
        assert!(is_noise(Path::new("/r/.git/objects/ab/cd")));
        assert!(is_noise(Path::new("/r/.git/logs/HEAD")));
        assert!(is_noise(Path::new("/r/.git/worktrees/x/logs/HEAD")));
        assert!(is_noise(Path::new("/r/.git/index.lock")));
        assert!(!is_noise(Path::new("/r/.git/index")));
        assert!(!is_noise(Path::new("/r/.git/worktrees/x/index")));
        assert!(!is_noise(Path::new("/r/src/main.rs")));
    }
}
