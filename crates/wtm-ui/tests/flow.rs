//! The whole UI, driven through the headless backend against real git in a
//! temporary folder: what a person would click, and what the window then
//! shows. A toolkit backend only has to draw what these views describe.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use wtm_core::{Action, App, AppSettings, Focus, UiState};
use wtm_platform::{AppDirs, NoUpdater, Platform};
use wtm_toolkit::headless::Harness;
use wtm_toolkit::{Dialog, Effect, Element, Frame, HostEvent, RowContent, View};
use wtm_ui::{Msg, Ui};

const WAIT: Duration = Duration::from_secs(20);

struct StubPlatform;

impl Platform for StubPlatform {
    fn spawn_detached(&self, _: &str, _: Option<&Path>) -> std::io::Result<()> {
        Ok(())
    }
    fn open_in_terminal(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn reveal(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn file_manager_name(&self) -> &'static str {
        "Files"
    }
}

/// Keep the developer's own git config out of the tests' git and the app's.
fn isolate_git_config() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    });
}

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repo `app` with a commit on `main` and a branch `dev`, in a fresh
/// folder that also holds the config and the worktrees root.
/// `path` as git writes it, which is how the UI shows and keys it.
fn shown(path: &std::path::Path) -> String {
    wtm_core::paths::normalise(&path.to_string_lossy())
}

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    isolate_git_config();
    let base = std::env::temp_dir().join(format!("wtm-ui-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    // macOS's temporary directory is under /var, a link to /private/var,
    // and git reports the resolved path; `shown` turns it into git's
    // spelling on Windows too.
    let base = base.canonicalize().unwrap();
    let repo = base.join("app");
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
            "first",
        ],
    );
    git(&repo, &["branch", "dev"]);
    (base, repo)
}

fn app_in(base: &Path) -> App {
    let dirs = AppDirs {
        config_dir: base.join("config"),
        home: base.to_path_buf(),
        legacy_config_files: vec![],
    };
    let app = App::new(Arc::new(StubPlatform), dirs);
    app.dispatch(Action::SetSettings(AppSettings {
        worktrees_root: base.join("wts").to_string_lossy().into_owned(),
        editor_command: "true".into(),
        update_channel: Default::default(),
    }));
    app
}

const SCREEN: Frame = Frame {
    x: 0.0,
    y: 0.0,
    width: 1600.0,
    height: 1000.0,
};

fn start(app: &App) -> Harness<Ui> {
    Harness::new(Ui::new(app.clone(), Rc::new(NoUpdater)), SCREEN)
}

/// Start the UI on a fixture with its repo added and listed.
fn listed(name: &str) -> (PathBuf, PathBuf, App, Harness<Ui>) {
    let (base, repo) = fixture(name);
    let app = app_in(&base);
    let h = start(&app);
    h.send(Msg::PickedRepos(vec![repo.clone()]));
    h.wait_for(WAIT, "the repo to be listed", |v| {
        v.window
            .list
            .sections
            .first()
            .is_some_and(|s| !s.header.loading && !s.rows.is_empty())
    });
    (base, repo, app, h)
}

fn section_key(v: &View) -> String {
    v.window.list.sections[0].key.clone()
}

fn row_keys(v: &View) -> Vec<String> {
    v.window.list.sections[0]
        .rows
        .iter()
        .map(|r| r.key.clone())
        .collect()
}

fn dialog(v: &View) -> &Dialog {
    v.dialogs.first().expect("a dialog is open")
}

fn body(d: &Dialog) -> &Element {
    d.body.as_ref().expect("the dialog has a body")
}

fn type_into(h: &Harness<Ui>, field: &'static str, text: &str) {
    let v = h.view();
    let d = dialog(&v);
    if let Some(f) = body(d).field(field) {
        h.fire(&f.on_change, text.to_string());
    } else if let Some(c) = body(d).combo(field) {
        h.fire(&c.on_change, text.to_string());
    } else {
        panic!("no field {field}");
    }
}

fn press(h: &Harness<Ui>, label: &str) {
    let v = h.view();
    let b = dialog(&v)
        .button(label)
        .unwrap_or_else(|| panic!("no button {label}"));
    assert!(b.enabled, "{label} is disabled");
    h.fire(&b.on_press, ());
    // The next dialog opens on the turn after this one closed.
    h.advance(Duration::ZERO);
}

#[test]
fn lists_a_repo_and_searches_it() {
    let (_base, repo, _app, h) = listed("search");
    let v = h.view();
    let s = &v.window.list.sections[0];
    assert_eq!(s.header.name, "app");
    assert_eq!(s.header.meta, "main · 1 worktree");
    assert!(s.expanded);
    let RowContent::Worktree(w) = &s.rows[0].content else {
        panic!("a worktree row")
    };
    assert_eq!(w.branch_name, "main");
    assert_eq!(w.path, "~/app");
    assert!(w.badges.iter().any(|b| b.text == "primary"));
    assert!(
        w.action("delete").unwrap().hidden,
        "the primary tree has no delete"
    );
    assert_eq!(w.action("reveal").unwrap().hint, "Reveal in Files");
    assert_eq!(s.header.path_full, shown(&repo));

    // A search keeps the repos with a match, and counts what it shows.
    h.send(Msg::Query("mai".into()));
    let v = h.view();
    assert_eq!(
        v.window.list.sections[0].header.meta,
        "main · 1 of 1 worktree"
    );
    h.send(Msg::Query("nothing-like-it".into()));
    assert!(h.view().window.list.sections.is_empty());
}

#[test]
fn a_search_opens_closed_cards_and_clearing_it_closes_them_again() {
    let (_base, _repo, _app, h) = listed("cards");
    let key = section_key(&h.view());
    h.send(Msg::Toggle(key.clone(), false));
    assert!(!h.view().window.list.sections[0].expanded);
    h.send(Msg::Query("main".into()));
    assert!(h.view().window.list.sections[0].expanded);
    h.send(Msg::Query(String::new()));
    assert!(!h.view().window.list.sections[0].expanded);
}

#[test]
fn a_card_closed_mid_search_stays_closed_while_typing() {
    let (_base, _repo, _app, h) = listed("search-close");
    let key = section_key(&h.view());
    h.send(Msg::Query("m".into()));
    h.send(Msg::Toggle(key.clone(), false));
    assert!(!h.view().window.list.sections[0].expanded);
    h.send(Msg::Query("ma".into()));
    assert!(!h.view().window.list.sections[0].expanded);
    h.send(Msg::Query(String::new()));
    assert!(
        h.view().window.list.sections[0].expanded,
        "open before the search, so open after it"
    );
}

#[test]
fn creates_a_worktree_from_the_sheet() {
    let (_base, _repo, app, h) = listed("create");
    let v = h.view();
    h.fire(&v.window.list.sections[0].header.on_new_worktree, ());
    let v = h.view();
    let d = dialog(&v);
    assert_eq!(d.title, "New worktree — app");
    assert_eq!(d.focus, Some("branch"));
    assert!(!d.button("Create").unwrap().enabled, "no name yet");

    // An existing branch is refused in New branch mode, and taken in the
    // other.
    type_into(&h, "branch", "dev");
    let v = h.view();
    assert_eq!(
        body(dialog(&v)).text("note").unwrap().content.to_plain(),
        "Branch already exists; use Existing branch."
    );
    let mode = body(dialog(&v))
        .segmented("mode")
        .unwrap()
        .on_select
        .clone();
    h.fire(&mode, 1);
    let v = h.view();
    assert!(dialog(&v).button("Create").unwrap().enabled);
    assert!(
        body(dialog(&v))
            .walk_rows()
            .iter()
            .any(|(caption, hidden)| caption == "Base ref:" && *hidden),
        "no base ref for an existing branch"
    );
    h.fire(&mode, 0);

    type_into(&h, "branch", "feature/one");
    let v = h.view();
    assert!(dialog(&v).button("Create").unwrap().enabled);
    press(&h, "Create");
    assert!(h.view().dialogs.is_empty());
    let v = h.wait_for(WAIT, "the new worktree", |v| {
        v.window.list.sections[0].rows.iter().any(
            |r| matches!(&r.content, RowContent::Worktree(w) if w.branch_name == "feature/one"),
        )
    });
    assert_eq!(row_keys(&v).len(), 2);
    assert!(app.model().pending.is_empty());
}

#[test]
fn create_sends_what_was_typed_before_the_click() {
    let (_base, _repo, app, h) = listed("typed");
    let v = h.view();
    h.fire(&v.window.list.sections[0].header.on_new_worktree, ());
    type_into(&h, "branch", "fo");
    let create = dialog(&h.view()).button("Create").unwrap().on_press.clone();
    // The last key and the click land in one batch, so the button pressed
    // is the one rendered before that key.
    type_into(&h, "branch", "foo");
    h.fire(&create, ());
    h.wait_for(WAIT, "the worktree named as typed", |v| {
        v.window.list.sections[0]
            .rows
            .iter()
            .any(|r| matches!(&r.content, RowContent::Worktree(w) if w.branch_name == "foo"))
    });
    assert!(app.model().repos[0]
        .worktrees
        .iter()
        .all(|w| w.branch.as_deref() != Some("fo")));
}

#[test]
fn a_dirty_worktree_is_deleted_only_after_a_second_question() {
    let (base, _repo, _app, h) = listed("delete");
    let v = h.view();
    h.fire(&v.window.list.sections[0].header.on_new_worktree, ());
    type_into(&h, "branch", "doomed");
    press(&h, "Create");
    h.wait_for(WAIT, "the worktree to delete", |v| {
        v.window
            .list
            .worktree(&format!("w:{}", shown(&base.join("wts/app/doomed"))))
            .is_some()
    });
    let key = format!("w:{}", shown(&base.join("wts/app/doomed")));
    std::fs::write(base.join("wts/app/doomed/scratch.txt"), "x").unwrap();
    h.send(Msg::Refresh);
    let v = h.wait_for(WAIT, "the untracked file to show", |v| {
        v.window
            .list
            .worktree(&key)
            .is_some_and(|w| w.badges.iter().any(|b| b.text == "untracked"))
    });
    let _ = v;

    let delete = h
        .view()
        .window
        .list
        .worktree(&key)
        .unwrap()
        .action("delete")
        .unwrap()
        .on_press
        .clone();
    h.fire(&delete, ());
    let v = h.view();
    assert_eq!(dialog(&v).title, "Delete worktree “doomed”?");
    assert!(dialog(&v).message.contains("uncommitted changes"));
    press(&h, "Delete");
    let v = h.wait_for(WAIT, "the second question", |v| {
        h.advance(Duration::ZERO);
        !v.dialogs.is_empty()
    });
    assert_eq!(dialog(&v).title, "“doomed” has uncommitted changes");
    press(&h, "Force Delete — Discard Changes");
    h.wait_for(WAIT, "the row to go", |v| {
        v.window.list.worktree(&key).is_none()
    });
    assert!(!base.join("wts/app/doomed").exists());
}

#[test]
fn repo_settings_save_and_remove() {
    let (_base, _repo, app, h) = listed("repo-settings");
    let v = h.view();
    h.fire(&v.window.list.sections[0].header.on_settings, ());
    let v = h.view();
    assert_eq!(dialog(&v).title, "Repo settings — app");
    assert_eq!(body(dialog(&v)).field("name").unwrap().value, "app");
    type_into(&h, "name", "  Renamed  ");
    type_into(&h, "init", "echo hi");
    press(&h, "Save");
    let v = h.wait_for(WAIT, "the new name", |v| {
        v.window.list.sections[0].header.name == "Renamed"
    });
    assert_eq!(app.model().repos[0].repo.init_command, "echo hi");

    h.fire(&v.window.list.sections[0].header.on_settings, ());
    press(&h, "Remove Repo…");
    let v = h.view();
    assert_eq!(dialog(&v).title, "Remove “Renamed” from the list?");
    press(&h, "Remove");
    let v = h.wait_for(WAIT, "the empty state", |v| v.window.empty.is_some());
    assert!(v.window.list.sections.is_empty());
}

#[test]
fn a_dialog_waits_for_the_one_before_it() {
    let (_base, _repo, _app, h) = listed("queue");
    let v = h.view();
    let header = &v.window.list.sections[0].header;
    h.fire(&header.on_settings, ());
    h.fire(&header.on_new_worktree, ());
    let v = h.view();
    assert_eq!(v.dialogs.len(), 2);
    assert_eq!(dialog(&v).title, "Repo settings — app");
    let cancel = dialog(&v).button("Cancel").unwrap().on_press.clone();
    h.fire(&cancel, ());
    assert!(h.view().dialogs.is_empty(), "nothing on the turn it closed");
    h.advance(Duration::ZERO);
    assert_eq!(dialog(&h.view()).title, "New worktree — app");

    // The folder picker waits for the dialogs too.
    h.clear_effects();
    h.send(Msg::AddRepo);
    assert!(!h
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::PickFolders(_))));
    press(&h, "Cancel");
    assert!(h
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::PickFolders(_))));
}

#[test]
fn the_picker_switches_branch() {
    let (_base, repo, _app, h) = listed("picker");
    let key = row_keys(&h.view())[0].clone();
    h.send(Msg::Select(Some(key.clone())));
    let (switch, enabled) = {
        let v = h.view();
        let (handler, enabled) = v.menu_item("Switch Branch…").unwrap();
        (handler.clone(), enabled)
    };
    assert!(enabled);
    h.fire(&switch, ());
    let v = h.view();
    let p = v.popover.as_ref().expect("the picker is open");
    assert_eq!(p.anchor, (key.clone(), wtm_toolkit::BRANCH_BUTTON));
    assert_eq!(p.list.items.len(), 2);
    assert!(p.list.items[p.list.selected.unwrap()].checked);
    h.fire(&p.list.on_query, "de".to_string());
    let v = h.view();
    let p = v.popover.as_ref().unwrap();
    assert_eq!(p.list.items[0].label.to_plain(), "dev");
    h.fire(&p.list.on_choose, 0);
    assert!(h.view().popover.is_none());
    assert!(h.effects().contains(&Effect::FocusList));
    h.wait_for(WAIT, "the switch", |v| {
        v.window
            .list
            .worktree(&key)
            .is_some_and(|w| w.branch_name == "dev")
    });
    assert_eq!(git(&repo, &["branch", "--show-current"]), "dev");
}

#[test]
fn a_stale_picker_message_is_ignored() {
    let (_base, _repo, _app, h) = listed("stale-picker");
    let key = row_keys(&h.view())[0].clone();
    h.send(Msg::Activate(key.clone()));
    let first = h.view().popover.unwrap();
    h.send(Msg::Activate(key));
    let second = h.view().popover.unwrap();
    assert_ne!(first.id, second.id);
    // AppKit reports the first one's close after the second has opened.
    h.fire(&first.list.on_dismiss, ());
    assert_eq!(h.view().popover.map(|p| p.id), Some(second.id));
}

#[test]
fn settings_keep_what_was_typed() {
    let (_base, _repo, app, h) = listed("settings");
    h.send(Msg::OpenSettings);
    let v = h.view();
    let panel = v.panel("settings").expect("the settings window");
    let editor = panel.body.field("editor").unwrap().on_change.clone();
    h.fire(&editor, "code ".to_string());
    // The trimmed value is applied; the field keeps the space.
    h.fire(&editor, "code -w ".to_string());
    h.wait_for(WAIT, "the editor setting", |_| {
        app.model().config.editor_command == "code -w"
    });
    let v = h.view();
    assert_eq!(
        v.panel("settings")
            .unwrap()
            .body
            .field("editor")
            .unwrap()
            .value,
        "code -w "
    );
    // An empty root is never applied.
    let root = v
        .panel("settings")
        .unwrap()
        .body
        .field("root")
        .unwrap()
        .on_change
        .clone();
    h.fire(&root, String::new());
    assert!(!app.model().config.worktrees_root.is_empty());
}

#[test]
fn menu_items_follow_the_selection() {
    let (base, _repo, app, h) = listed("menus");
    let enabled = |label: &str| h.view().menu_item(label).unwrap().1;
    assert!(
        enabled("New Worktree…"),
        "the first repo, with nothing selected"
    );
    assert!(!enabled("Switch Branch…"));
    assert!(!enabled("Move Repository Up"));

    // A second repo to move.
    let other = base.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    h.send(Msg::PickedRepos(vec![other]));
    h.wait_for(WAIT, "two repos", |v| v.window.list.sections.len() == 2);
    let second = h.view().window.list.sections[1].key.clone();
    h.send(Msg::Select(Some(second)));
    assert!(enabled("Move Repository Up"));
    assert!(!enabled("Move Repository Down"));
    let (up, _) = {
        let v = h.view();
        let (handler, e) = v.menu_item("Move Repository Up").unwrap();
        (handler.clone(), e)
    };
    h.fire(&up, ());
    h.wait_for(WAIT, "the move", |v| {
        v.window.list.sections[0].header.name == "other"
    });
    assert_eq!(app.model().config.repos[0].name, "other");
    assert!(h.effects().contains(&Effect::RevealSelection));
}

#[test]
fn a_drop_names_the_repo_it_lands_before() {
    let (base, _repo, app, h) = listed("drop");
    let other = base.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    h.send(Msg::PickedRepos(vec![other]));
    let v = h.wait_for(WAIT, "two repos", |v| v.window.list.sections.len() == 2);
    let (first, second) = (
        v.window.list.sections[0].key.clone(),
        v.window.list.sections[1].key.clone(),
    );
    let reorder = v.window.list.on_reorder.clone().unwrap();
    // Beside itself: nothing.
    h.fire(&reorder, (first.clone(), Some(second.clone())));
    assert_eq!(app.model().config.repos[0].name, "app");
    h.fire(&reorder, (first, None));
    h.wait_for(WAIT, "the move", |v| {
        v.window.list.sections[1].header.name == "app"
    });
}

#[test]
fn the_window_comes_back_as_it_was() {
    let (base, _repo, app, h) = listed("restore");
    let key = row_keys(&h.view())[0].clone();
    let section = section_key(&h.view());
    h.send(Msg::Select(Some(key.clone())));
    h.host(HostEvent::Scrolled(40.0));
    h.host(HostEvent::FrameChanged(Frame {
        x: 100.0,
        y: 100.0,
        width: 900.0,
        height: 600.0,
    }));
    h.host(HostEvent::WillQuit);
    assert_eq!(
        app.ui_state().focus,
        Some(Focus {
            repo_id: app.model().repos[0].repo.id.clone(),
            worktree_path: key.strip_prefix("w:").map(Into::into),
        })
    );
    drop(h);
    drop(app);

    let app = app_in(&base);
    let h = start(&app);
    let v = h.view();
    assert_eq!(
        v.window.frame,
        Some(Frame {
            x: 100.0,
            y: 100.0,
            width: 900.0,
            height: 600.0
        })
    );
    // Restored once the cards hold their worktrees: from the snapshot when
    // it was written in time, else from the first listing.
    h.wait_for(WAIT, "the focused row back", |v| {
        v.window.list.selected.as_ref() == Some(&key)
    });
    assert!(h.effects().contains(&Effect::ScrollTo(40.0)));
    assert!(v.window.list.section(&section).is_some());

    // A frame on a screen that has gone is not used.
    let state = UiState {
        window: Some(wtm_core::WindowFrame {
            x: 5000.0,
            y: 0.0,
            width: 900.0,
            height: 600.0,
        }),
        ..app.ui_state()
    };
    app.store_ui_state(state);
    app.flush_ui_state();
    drop(h);
    let h = start(&app);
    assert_eq!(h.view().window.frame, None);
}

#[test]
fn notices_clear_themselves_and_offer_details() {
    let (_base, _repo, app, h) = listed("notice");
    let long: String = (0..10).map(|i| format!("line {i}\n")).collect();
    app.dispatch(Action::ShowNotice {
        text: long.clone(),
        tone: wtm_core::model::Tone::Info,
    });
    let v = h.wait_for(WAIT, "the notice", |v| v.window.notice.is_some());
    let bar = v.window.notice.as_ref().unwrap();
    assert_eq!(
        bar.text("notice").unwrap().content.to_plain(),
        "line 0\nline 1 … (8 more lines)"
    );
    let details = bar.button("details").unwrap();
    assert!(!details.hidden);
    assert!(bar.button("restart").unwrap().hidden);
    h.fire(&details.on_press, ());
    let v = h.view();
    assert_eq!(dialog(&v).title, "line 0");
    assert_eq!(dialog(&v).body, Some(Element::TextBlock(long)));
    press(&h, "OK");

    // The timer fires after five seconds.
    h.advance(Duration::from_secs(6));
    h.wait_for(WAIT, "the notice to clear", |v| v.window.notice.is_none());
}

/// Captions of a form's rows and whether each is hidden.
trait Rows {
    fn walk_rows(&self) -> Vec<(String, bool)>;
}

impl Rows for Element {
    fn walk_rows(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        self.walk(&mut |e| {
            if let Element::Form(f) = e {
                out.extend(f.rows.iter().map(|r| (r.caption.clone(), r.hidden)));
            }
        });
        out
    }
}
