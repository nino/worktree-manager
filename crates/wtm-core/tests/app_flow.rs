//! End-to-end test of the core against a real temporary git repository: add
//! a repo, list it, create a worktree, exercise the delete safety ladder, and
//! confirm the model events arrive. No UI involved; the platform is a stub.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wtm_core::{Action, App, CreateWorktreeParams, DeleteRefusal, DeleteWorktreeParams, Event};
use wtm_platform::{AppDirs, Platform};

struct StubPlatform;

impl Platform for StubPlatform {
    fn spawn_detached(&self, _: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn open_in_terminal(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn reveal(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn file_manager_name(&self) -> &'static str {
        "Stub"
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("wtm-core-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

/// Wait until `pred` holds on the model, driven by change events.
fn wait_for(
    app: &App,
    rx: &mpsc::Receiver<Event>,
    what: &str,
    pred: impl Fn(&wtm_core::Model) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if pred(&app.model()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:#?}",
            app.model()
        );
        let _ = rx.recv_timeout(Duration::from_millis(200));
    }
}

#[test]
fn add_repo_create_and_delete_worktree() {
    let base = temp_dir("flow");
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    let repo = std::fs::canonicalize(&repo).unwrap();

    let dirs = AppDirs {
        config_dir: base.join("config"),
        home: base.clone(),
        legacy_config_files: vec![],
    };
    let app = App::new(Arc::new(StubPlatform), dirs);
    let (tx, rx) = mpsc::channel();
    app.subscribe(move |e| {
        let _ = tx.send(e);
    });
    app.dispatch(Action::SetSettings(wtm_core::AppSettings {
        worktrees_root: base.join("wts").to_string_lossy().into_owned(),
        editor_command: "true".into(),
        update_channel: Default::default(),
    }));

    // Add the repo by a path inside it (a file), exercising root resolution.
    app.dispatch(Action::AddRepos(vec![repo.join(".git").join("HEAD")]));
    wait_for(&app, &rx, "repo listed", |m| {
        m.repos.len() == 1 && m.repos[0].loaded
    });
    let model = app.model();
    let node = &model.repos[0];
    assert_eq!(node.repo.path, repo.to_string_lossy());
    assert_eq!(node.repo.main_branch, "main");
    assert_eq!(node.worktrees.len(), 1);
    assert!(node.worktrees[0].is_main);
    assert_eq!(node.default_base_ref, "main");
    let repo_id = node.repo.id.clone();
    assert!(matches!(&model.notice, Some(n) if n.text.starts_with("Added repo")));

    // Adding again is refused per path, not fatal.
    app.dispatch(Action::AddRepos(vec![repo.clone()]));
    wait_for(&app, &rx, "duplicate notice", |m| {
        m.notice
            .as_ref()
            .map(|n| n.text.contains("Already added"))
            .unwrap_or(false)
    });
    assert_eq!(app.model().repos.len(), 1);

    // Create a worktree on a new branch: a placeholder appears immediately.
    app.dispatch(Action::CreateWorktree(CreateWorktreeParams {
        repo_id: repo_id.clone(),
        branch: "feature/x".into(),
        new_branch: true,
        base_ref: None,
    }));
    assert_eq!(
        app.model().pending.len(),
        1,
        "placeholder row is synchronous"
    );
    wait_for(&app, &rx, "worktree created", |m| {
        m.pending.is_empty() && m.repos[0].worktrees.len() == 2
    });
    let model = app.model();
    let wt = model.repos[0]
        .worktrees
        .iter()
        .find(|w| !w.is_main)
        .unwrap()
        .clone();
    assert_eq!(wt.branch.as_deref(), Some("feature/x"));
    assert!(wt.path.ends_with("/repo/feature-x"));
    let status = wt.status.clone().expect("status computed");
    assert!(!status.is_dirty());
    assert_eq!(status.ahead_of_main, Some(0));
    assert!(
        !status.has_upstream,
        "--no-track must leave the branch without upstream"
    );

    // A bad creation lands as an error on its placeholder.
    app.dispatch(Action::CreateWorktree(CreateWorktreeParams {
        repo_id: repo_id.clone(),
        branch: "feature/x".into(),
        new_branch: true,
        base_ref: None,
    }));
    wait_for(&app, &rx, "creation error", |m| {
        m.pending.iter().any(|p| p.error.is_some())
    });
    let id = app.model().pending[0].id;
    app.dispatch(Action::DismissCreation(id));
    assert!(app.model().pending.is_empty());

    // Dirty the worktree; the file watcher (or the explicit refresh) sees it.
    std::fs::write(Path::new(&wt.path).join("new.txt"), "hi").unwrap();
    app.dispatch(Action::RefreshWorktree {
        repo_id: repo_id.clone(),
        path: wt.path.clone(),
    });
    wait_for(&app, &rx, "untracked status", |m| {
        m.repos[0]
            .worktree(&wt.path)
            .and_then(|w| w.status.as_ref())
            .map(|s| s.has_untracked)
            .unwrap_or(false)
    });

    // Delete without force is refused as dirty; the row is busy meanwhile.
    let (dtx, drx) = mpsc::channel();
    app.dispatch(Action::DeleteWorktree(
        DeleteWorktreeParams {
            repo_id: repo_id.clone(),
            worktree_path: wt.path.clone(),
            expected_branch: wt.branch.clone(),
            force: false,
        },
        Box::new(move |r| {
            let _ = dtx.send(r);
        }),
    ));
    assert!(app.model().busy_for(&wt.path).is_some());
    let result = drx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(!result.ok);
    assert_eq!(result.reason, Some(DeleteRefusal::Dirty));

    // Stale expectation is refused as changed.
    let (dtx, drx) = mpsc::channel();
    app.dispatch(Action::DeleteWorktree(
        DeleteWorktreeParams {
            repo_id: repo_id.clone(),
            worktree_path: wt.path.clone(),
            expected_branch: Some("other".into()),
            force: true,
        },
        Box::new(move |r| {
            let _ = dtx.send(r);
        }),
    ));
    let result = drx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(result.reason, Some(DeleteRefusal::Changed));

    // The primary tree can never be deleted.
    let main_path = app.model().repos[0]
        .worktrees
        .iter()
        .find(|w| w.is_main)
        .unwrap()
        .path
        .clone();
    let (dtx, drx) = mpsc::channel();
    app.dispatch(Action::DeleteWorktree(
        DeleteWorktreeParams {
            repo_id: repo_id.clone(),
            worktree_path: main_path,
            expected_branch: Some("main".into()),
            force: true,
        },
        Box::new(move |r| {
            let _ = dtx.send(r);
        }),
    ));
    let result = drx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(result.reason, Some(DeleteRefusal::Error));
    assert!(result.message.contains("primary"));

    // Force delete succeeds and the tree drops the row.
    let (dtx, drx) = mpsc::channel();
    app.dispatch(Action::DeleteWorktree(
        DeleteWorktreeParams {
            repo_id: repo_id.clone(),
            worktree_path: wt.path.clone(),
            expected_branch: wt.branch.clone(),
            force: true,
        },
        Box::new(move |r| {
            let _ = dtx.send(r);
        }),
    ));
    let result = drx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(result.ok, "{}", result.message);
    wait_for(&app, &rx, "row gone", |m| {
        m.repos[0].worktrees.len() == 1 && m.busy.is_empty()
    });
    assert!(!Path::new(&wt.path).exists());

    // Push with no remote fails softly into the notice, never a panic.
    let main_path = app.model().repos[0].worktrees[0].path.clone();
    app.dispatch(Action::Push {
        repo_id: repo_id.clone(),
        path: main_path,
    });
    wait_for(&app, &rx, "push failure notice", |m| {
        m.busy.is_empty()
            && m.notice
                .as_ref()
                .map(|n| n.text.starts_with("Pushing failed"))
                .unwrap_or(false)
    });

    // Config and snapshot were persisted for the next launch.
    assert!(base.join("config/config.json").exists());
    assert!(base.join("config/snapshot.json").exists());
    app.dispatch(Action::RemoveRepo(repo_id));
    assert!(app.model().repos.is_empty());
    let _ = std::fs::remove_dir_all(&base);
}
