//! Worktree Manager's UI, written once against `wtm-toolkit` and shown by
//! whichever backend the executable picks: AppKit, GTK or Win32.
//!
//! [`Ui`] is the [`Program`]: it holds the model snapshot it last showed and
//! everything the window adds to it (the search, which cards are closed, the
//! selection, open dialogs and their drafts, the branch picker), turns
//! messages into core [`Action`]s, and builds the [`View`] from all of it.
//! Nothing here names a toolkit or an operating system.

pub mod dialogs;
pub mod list;
pub mod menus;
pub mod picker;
pub mod settings;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wtm_core::model::Tone;
use wtm_core::repos::drop_target;
use wtm_core::{Action, App, DeleteRefusal, Focus, Model, UiState, WindowFrame};
use wtm_platform::Updater;
use wtm_toolkit::{
    Align, Button, ButtonKind, Cx, Effect, Element, Frame, HostEvent, Icon, Ink, Key, MainWindow,
    PickFolders, Program, Spinner, Stack, Text, TextStyle, ToolItem, Toolkit, TreeList, View,
    ViewCx,
};

use dialogs::{Kind, Open, Outcome};
use list::{Item, Op};
use picker::{Picker, PickerMsg};
use settings::{Settings, SettingsMsg};

/// Run the app's UI on `toolkit`. Never returns.
pub fn run(toolkit: impl Toolkit, app: App, updater: Rc<dyn Updater>) -> ! {
    toolkit.run(Ui::new(app, updater))
}

/// How long an informational notice stays before it clears itself.
const NOTICE_LIFETIME: Duration = Duration::from_secs(5);

/// Activations closer together than this do not refresh (Cmd-Tab flicker).
const ACTIVATION_REFRESH_GAP: Duration = Duration::from_secs(2);

/// Everything the UI is told. Messages carry what the user saw when they
/// acted, not what to look up later (see `dialogs`).
#[derive(Debug, Clone)]
pub enum Msg {
    /// The core published a new snapshot.
    ModelChanged,
    /// The updater has something new to offer, or nothing any more.
    UpdaterChanged,

    // The list.
    Query(String),
    Select(Option<Key>),
    Toggle(Key, bool),
    Reorder(Key, Option<Key>),
    Activate(Key),

    // Toolbar and menus.
    AddRepo,
    PickedRepos(Vec<PathBuf>),
    Refresh,
    OpenSettings,
    /// ⌘N: in the selected repo.
    NewWorktree,
    /// ⌘T: the picker for the selected worktree.
    SwitchBranch,
    MoveRepo {
        up: bool,
    },
    FocusSearch,
    CheckForUpdates,
    RestartForUpdate,

    // The notice bar.
    DismissNotice,
    NoticeDetails,
    NoticeExpired(u64),

    // Rows.
    NewWorktreeIn(String),
    RepoSettings(String),
    Copy(String),
    Op {
        op: Op,
        repo_id: String,
        path: String,
    },
    Delete {
        repo_id: String,
        path: String,
    },
    DeleteDone {
        params: wtm_core::DeleteWorktreeParams,
        reason: Option<DeleteRefusal>,
        message: String,
    },
    DismissCreation(u64),
    OpenPicker(Key),
    Picker(u64, PickerMsg),

    // Dialogs.
    DialogField(u64, wtm_toolkit::Id, String),
    DialogMode(u64, usize),
    DialogDone(u64, Outcome),
    /// The turn after a dialog closed: the next one may open.
    DialogGate,
    /// Whether something is at a folder a New Worktree sheet asked about.
    FolderLooked {
        path: String,
        there: bool,
    },

    Settings(SettingsMsg),
}

pub struct Ui {
    app: App,
    updater: Rc<dyn Updater>,
    file_manager: &'static str,
    terminal_note: &'static str,
    /// The snapshot on screen.
    model: Arc<Model>,
    /// A model-changed message is queued and not yet handled; bursts from
    /// the core's threads become one.
    model_queued: Arc<AtomicBool>,

    query: String,
    /// Repos whose cards are closed.
    collapsed: HashSet<String>,
    /// `collapsed` as it was before the current search, which opens every
    /// card; put back when the search is cleared. `None` when not searching.
    collapsed_before_search: Option<HashSet<String>>,
    selected: Option<Key>,
    scroll: f64,
    /// The window's frame as last reported, else as it was last launch.
    frame: Option<WindowFrame>,
    /// Where the window opens, worked out once the screens are known.
    initial_frame: Option<Frame>,
    /// The scroll offset and focused row this launch is coming back to,
    /// until there are rows to put them back on. Until then they are
    /// recorded in place of the list's own: it sits at the top with nothing
    /// selected, which is not what should be remembered.
    restore: Option<(f64, Option<Focus>)>,

    /// The notice the auto-clear timer was last started for.
    notice_timed: u64,
    last_activation: Option<Instant>,

    /// Modal questions in the order asked; the first is on screen.
    dialogs: Vec<Open>,
    /// A dialog closed this turn; the next opens on the following one, as a
    /// sheet must finish leaving before another can come.
    dialog_gate: bool,
    /// The Add Repository picker, waiting for the dialogs to close.
    pick_waiting: bool,
    picker: Option<Picker>,
    settings: Option<Settings>,
    next_id: u64,

    /// Whether something was at each worktree folder a sheet asked about,
    /// when last looked; and the folders being looked at now, so a hung
    /// volume does not pile up looks.
    folders: HashMap<String, bool>,
    looking: HashSet<String>,
}

impl Ui {
    pub fn new(app: App, updater: Rc<dyn Updater>) -> Self {
        let model = app.model();
        let state = app.ui_state();
        let file_manager = app.platform().file_manager_name();
        let terminal_note = app.platform().terminal_note();
        Ui {
            app,
            updater,
            file_manager,
            terminal_note,
            model,
            model_queued: Arc::new(AtomicBool::new(false)),
            query: String::new(),
            collapsed: state.collapsed_repos.iter().cloned().collect(),
            collapsed_before_search: None,
            selected: None,
            scroll: 0.0,
            frame: state.window,
            initial_frame: None,
            restore: Some((state.scroll, state.focus)),
            notice_timed: 0,
            last_activation: None,
            dialogs: Vec::new(),
            dialog_gate: false,
            pick_waiting: false,
            picker: None,
            settings: None,
            next_id: 1,
            folders: HashMap::new(),
            looking: HashSet::new(),
        }
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn searching(&self) -> bool {
        !list::normalise(&self.query).is_empty()
    }

    // MARK: Selection

    fn selected_item(&self) -> Option<Item> {
        Item::find(self.selected.as_deref()?, &self.model)
    }

    /// The selected repo, or the first one when nothing is selected.
    fn selected_repo_id(&self) -> Option<String> {
        match self.selected_item() {
            Some(item) => Some(item.repo_id().to_string()),
            None => self.model.repos.first().map(|r| r.repo.id.clone()),
        }
    }

    /// The selected repo, if a worktree can be created in it: not one whose
    /// listing failed, as the card's own New Worktree button is disabled for.
    fn creatable_repo_id(&self) -> Option<String> {
        let repo_id = self.selected_repo_id()?;
        self.model
            .repo(&repo_id)
            .is_some_and(|n| n.error.is_none())
            .then_some(repo_id)
    }

    fn selected_worktree(&self) -> Option<(String, String)> {
        match self.selected_item()? {
            Item::Worktree { repo_id, path } => Some((repo_id, path)),
            _ => None,
        }
    }

    fn visible_keys(&self) -> Vec<Key> {
        list::visible_keys(&self.model, &list::normalise(&self.query), |id| {
            !self.collapsed.contains(id)
        })
    }

    /// Drop a selection whose row has gone, so ⌘N and ⌘T never act on
    /// something no longer on screen.
    fn prune_selection(&mut self) {
        if let Some(key) = &self.selected {
            if !self.visible_keys().contains(key) {
                self.selected = None;
            }
        }
    }

    // MARK: Moving repos

    /// Where `repo_id` goes when dropped before `before` among the repos the
    /// list shows, as the repo it then goes before (`None` for last). The
    /// outer `None` is a move that would change nothing.
    fn repo_move(&self, repo_id: &str, before: Option<&str>) -> Option<Option<String>> {
        let query = list::normalise(&self.query);
        let all: Vec<&str> = self
            .model
            .repos
            .iter()
            .map(|r| r.repo.id.as_str())
            .collect();
        let shown = list::shown_repo_ids(&self.model, &query);
        let gap = match before {
            Some(b) => shown.iter().position(|s| *s == b)?,
            None => shown.len(),
        };
        drop_target(&all, &shown, repo_id, gap).map(|b| b.map(str::to_string))
    }

    /// The selected card moved one place up or down.
    fn selected_repo_move(&self, up: bool) -> Option<(String, Option<String>)> {
        let repo_id = self.selected_item()?.repo_id().to_string();
        let query = list::normalise(&self.query);
        let shown = list::shown_repo_ids(&self.model, &query);
        let from = shown.iter().position(|s| *s == repo_id)?;
        let gap = if up { from.checked_sub(1)? } else { from + 2 };
        if gap > shown.len() {
            return None;
        }
        let before = shown.get(gap).copied();
        Some((repo_id.clone(), self.repo_move(&repo_id, before)?))
    }

    // MARK: Window state

    /// Record where the window is, how far the list is scrolled, which row
    /// has the keyboard and which cards are closed. The core discards an
    /// unchanged value and coalesces the rest into one write.
    fn remember(&self) {
        let (scroll, focus) = match &self.restore {
            Some(r) => r.clone(),
            None => (self.scroll, self.selected_item().and_then(|i| i.focus())),
        };
        self.app.store_ui_state(UiState {
            window: self.frame,
            scroll,
            focus,
            // Mid-search every card is open; what is remembered is how they
            // were before it.
            collapsed_repos: self
                .collapsed_before_search
                .as_ref()
                .unwrap_or(&self.collapsed)
                .iter()
                .cloned()
                .collect(),
        });
    }

    /// Put the list back where it was, once there is a list to put back:
    /// each card has to hold its worktrees, from the cached snapshot or from
    /// the listing that replaces it. Taken, not read: a later listing must
    /// never move the list under someone already using it.
    fn try_restore(&mut self, cx: &mut Cx<Msg>) {
        if self.restore.is_none()
            || !self
                .model
                .repos
                .iter()
                .all(|r| r.loaded || !r.worktrees.is_empty())
        {
            return;
        }
        let Some((offset, focus)) = self.restore.take() else {
            return;
        };
        // Nothing is found for a worktree that is gone, or one inside a card
        // that is closed; either way the list starts unselected.
        if let Some(key) = focus.map(|f| Item::from_focus(&f).key()) {
            if self.visible_keys().contains(&key) {
                self.selected = Some(key);
            }
        }
        // Applied by the backend once the rows are laid out, clamped to the
        // list's height: it may be shorter than it was.
        if offset > 0.0 {
            cx.effect(Effect::ScrollTo(offset));
        }
    }

    // MARK: Model changes

    fn model_changed(&mut self, cx: &mut Cx<Msg>) {
        self.model_queued.store(false, Ordering::Release);
        self.model = self.app.model();
        self.prune_selection();
        self.try_restore(cx);
        // A picker for a worktree that has gone, or is now busy, closes.
        if let Some(p) = &self.picker {
            let gone = self
                .model
                .repo(&p.repo_id)
                .and_then(|n| n.worktree(&p.path))
                .is_none_or(|w| w.prunable || self.model.busy_for(&w.path).is_some());
            if gone {
                self.picker = None;
            }
        }
        self.recheck_sheets(cx);
        self.time_notice(cx);
    }

    /// Informational notices fade once the list shows the outcome. An
    /// update that is installed, or waiting to be, stays on offer until it
    /// is taken.
    fn time_notice(&mut self, cx: &mut Cx<Msg>) {
        let Some(n) = &self.model.notice else { return };
        if n.id == self.notice_timed {
            return;
        }
        self.notice_timed = n.id;
        let update =
            self.updater.ready_version().is_some() || self.updater.pending_version().is_some();
        if n.tone == Tone::Info && !update {
            cx.effect(Effect::After(
                NOTICE_LIFETIME,
                cx.on(Msg::NoticeExpired(n.id)),
            ));
        }
    }

    // MARK: Folders under the worktrees root

    /// The folder a worktree for the sheet's name would get, abbreviated,
    /// when something is already there as of the last look. That look
    /// happens off the main thread (a worktrees root on a network volume can
    /// take seconds to answer), and an answer that changes what was known
    /// checks every open sheet again.
    fn taken(&mut self, target: Option<String>, cx: &mut Cx<Msg>) -> Option<String> {
        let target = target?;
        let shown = self
            .folders
            .get(&target)
            .copied()
            .unwrap_or(false)
            .then(|| wtm_core::paths::tildify(&target, &self.model.home));
        if self.looking.insert(target.clone()) {
            let proxy = cx.proxy();
            std::thread::spawn(move || {
                let there = std::fs::symlink_metadata(&target).is_ok();
                proxy.send(Msg::FolderLooked {
                    path: target,
                    there,
                });
            });
        }
        shown
    }

    /// Check every open New Worktree sheet again.
    fn recheck_sheets(&mut self, cx: &mut Cx<Msg>) {
        for i in 0..self.dialogs.len() {
            if let Kind::NewWorktree(n) = &self.dialogs[i].kind {
                let target = if n.name.trim().is_empty() {
                    None
                } else {
                    n.target(&self.model)
                };
                let taken = self.taken(target, cx);
                if let Kind::NewWorktree(n) = &mut self.dialogs[i].kind {
                    n.recheck(&self.model, taken.as_deref());
                }
            }
        }
    }

    // MARK: Dialogs

    fn ask(&mut self, kind: Kind) {
        let id = self.next_id();
        self.dialogs.push(Open { id, kind });
    }

    fn open_new_worktree(&mut self, repo_id: &str, cx: &mut Cx<Msg>) {
        let Some(sheet) = dialogs::NewWorktree::new(&self.model, repo_id) else {
            return;
        };
        self.ask(Kind::NewWorktree(sheet));
        self.recheck_sheets(cx);
        // The remote branches known now are as of the last fetch, which can
        // be minutes old; a branch pushed since would be taken for a new one.
        // Fetch, and check the name again when the listing lands.
        self.app.dispatch(Action::FetchRepo(repo_id.to_string()));
    }

    fn dialog_done(&mut self, id: u64, outcome: Outcome, cx: &mut Cx<Msg>) {
        let Some(at) = self.dialogs.iter().position(|d| d.id == id) else {
            return;
        };
        self.dialogs.remove(at);
        self.dialog_gate = true;
        cx.effect(Effect::After(Duration::ZERO, cx.on(Msg::DialogGate)));
        // Every prefix typed left an entry; the next sheet looks afresh.
        if !self
            .dialogs
            .iter()
            .any(|d| matches!(d.kind, Kind::NewWorktree(_)))
        {
            self.folders.clear();
        }
        match outcome {
            Outcome::Close => {}
            Outcome::Create(params) => self.app.dispatch(Action::CreateWorktree(params)),
            Outcome::SaveRepo(repo) => self.app.dispatch(Action::UpdateRepo(repo)),
            Outcome::AskRemove { repo_id, name } => {
                self.ask(Kind::ConfirmRemove { repo_id, name });
            }
            Outcome::Remove(repo_id) => self.app.dispatch(Action::RemoveRepo(repo_id)),
            Outcome::Delete(params) => {
                let proxy = cx.proxy();
                let sent = params.clone();
                self.app.dispatch(Action::DeleteWorktree(
                    params,
                    Box::new(move |result| {
                        if !result.ok {
                            proxy.send(Msg::DeleteDone {
                                params: sent,
                                reason: result.reason,
                                message: result.message,
                            });
                        }
                    }),
                ));
            }
        }
    }

    /// A refused delete: a dirty tree asks again, louder; anything else
    /// (the row was stale, the worktree gone) says why and brings the list
    /// back in line with git.
    fn delete_refused(
        &mut self,
        params: wtm_core::DeleteWorktreeParams,
        reason: Option<DeleteRefusal>,
        message: String,
    ) {
        match reason {
            Some(DeleteRefusal::Dirty) if !params.force => self.ask(dialogs::force_delete(params)),
            _ => {
                self.ask(Kind::Error {
                    title: "Could not delete worktree".into(),
                    message,
                });
                self.app.dispatch(Action::RefreshRepo(params.repo_id));
            }
        }
    }

    /// The Add Repository picker opens once no dialog is up: both hang off
    /// the main window, which shows one at a time.
    fn pick_repos_if_free(&mut self, cx: &mut Cx<Msg>) {
        if self.pick_waiting && self.dialogs.is_empty() && !self.dialog_gate {
            self.pick_waiting = false;
            cx.effect(Effect::PickFolders(PickFolders {
                title: "Select git repositories".into(),
                multiple: true,
                parent: None,
                on_done: cx.map(Msg::PickedRepos),
            }));
        }
    }

    // MARK: The picker

    fn open_picker(&mut self, key: &str) {
        let Some(Item::Worktree { repo_id, path }) = Item::find(key, &self.model) else {
            return;
        };
        let model = self.model.clone();
        let Some(node) = model.repo(&repo_id) else {
            return;
        };
        let Some(w) = node.worktree(&path) else {
            return;
        };
        // The branch button is off for these.
        if w.prunable || self.model.busy_for(&path).is_some() {
            return;
        }
        let id = self.next_id();
        self.picker = Some(Picker::new(
            id,
            key.to_string(),
            repo_id.clone(),
            path.clone(),
            &node.branches,
            w.branch.clone(),
        ));
    }

    fn picker_msg(&mut self, id: u64, msg: PickerMsg, cx: &mut Cx<Msg>) {
        // A late message from a picker that has since been replaced.
        let Some(p) = self.picker.as_mut().filter(|p| p.id == id) else {
            return;
        };
        let choose = |p: &Picker, row: usize| {
            p.choice(row)
                .map(|branch| (p.repo_id.clone(), p.path.clone(), branch))
        };
        let chosen = match msg {
            PickerMsg::Query(q) => {
                p.set_query(q);
                return;
            }
            PickerMsg::Move(by) => {
                p.move_by(by);
                return;
            }
            PickerMsg::Choose(row) => choose(p, row),
            PickerMsg::Dismiss => None,
        };
        self.picker = None;
        // The popover took the keyboard; hand it back to the list rather
        // than leaving the window with nothing focused.
        cx.effect(Effect::FocusList);
        if let Some((repo_id, path, branch)) = chosen {
            self.app.dispatch(Action::Switch {
                repo_id,
                path,
                branch,
            });
        }
    }

    // MARK: Settings

    fn settings_msg(&mut self, msg: SettingsMsg, cx: &mut Cx<Msg>) {
        let Some(s) = self.settings.as_mut() else {
            return;
        };
        match msg {
            SettingsMsg::Root(v) => s.root = v,
            SettingsMsg::Editor(v) => s.editor = v,
            SettingsMsg::Channel(i) => s.channel = i,
            SettingsMsg::Browse => {
                cx.effect(Effect::PickFolders(PickFolders {
                    title: "Choose worktrees root folder".into(),
                    multiple: false,
                    parent: Some(settings::KEY.into()),
                    on_done: cx.map(|p| Msg::Settings(SettingsMsg::Picked(p))),
                }));
                return;
            }
            SettingsMsg::Picked(paths) => match paths.first() {
                Some(p) => s.root = p.to_string_lossy().into_owned(),
                None => return,
            },
            SettingsMsg::Closed => {
                self.settings = None;
                return;
            }
        }
        let Some(changed) = s.changed(&self.model.config) else {
            return;
        };
        // Switching the channel takes effect at once: the new channel's feed
        // is read straight away rather than at the next periodic check.
        let channel = changed.update_channel != self.model.config.update_channel;
        self.app.dispatch(Action::SetSettings(changed));
        if channel {
            self.updater.channel_changed();
        }
    }

    // MARK: Building the view

    fn toolbar(&self, v: &ViewCx<Msg>) -> Vec<ToolItem> {
        let button = |id, label: &str, icon, msg| ToolItem::Button {
            id,
            label: label.into(),
            icon,
            on_press: v.on(msg),
        };
        vec![
            button("add", "Add Repo", Icon::Add, Msg::AddRepo),
            button("refresh", "Refresh", Icon::Refresh, Msg::Refresh),
            ToolItem::Flex,
            ToolItem::Search {
                id: "search",
                value: self.query.clone(),
                placeholder: "Search worktrees".into(),
                on_change: v.map(Msg::Query),
            },
            button("settings", "Settings", Icon::Settings, Msg::OpenSettings),
        ]
    }

    fn notice(&self, v: &ViewCx<Msg>) -> Option<Element> {
        let n = self.model.notice.as_ref()?;
        let ready = self.updater.ready_version();
        let pending = self.updater.pending_version();
        let text = Text::new(list::notice_summary(&n.text))
            .id("notice")
            .style(TextStyle::Small)
            .ink(match n.tone {
                Tone::Error => Ink::Error,
                Tone::Info => Ink::Primary,
            })
            .tooltip(n.text.clone())
            // A notice must never resize the window: git output can run to
            // hundreds of lines. Two here; the rest behind "Details…".
            .lines(2)
            .wrap()
            .selectable()
            .grow();
        Some(
            Stack::row(6.0)
                .child(text)
                .child(Element::Gap(8.0))
                .child(
                    Button::new(
                        "details",
                        ButtonKind::Accessory,
                        "Details…",
                        v.on(Msg::NoticeDetails),
                    )
                    .hidden(!list::notice_is_long(&n.text)),
                )
                .child(
                    // An update still waiting on an administrator is put in
                    // place by this button, not just switched to, and the
                    // label is what warns that a password will be asked for.
                    Button::new(
                        "restart",
                        ButtonKind::Small,
                        if ready.is_some() {
                            "Restart"
                        } else {
                            "Install and Restart"
                        },
                        v.on(Msg::RestartForUpdate),
                    )
                    .hidden(ready.is_none() && pending.is_none()),
                )
                .child(Element::Gap(4.0))
                .child(Button::icon(
                    "dismiss",
                    Icon::Close,
                    "Dismiss",
                    v.on(Msg::DismissNotice),
                ))
                .into(),
        )
    }

    fn empty(&self, v: &ViewCx<Msg>) -> Option<Element> {
        if !self.model.repos.is_empty() {
            return None;
        }
        Some(
            Stack::column(10.0)
                .align(Align::Center)
                .child(Text::new("No repositories yet").style(TextStyle::Heading))
                .child(
                    Text::new("Add a git repository to see its worktrees here.")
                        .style(TextStyle::Small)
                        .ink(Ink::Secondary),
                )
                .child(
                    Button::new(
                        "add-repo",
                        ButtonKind::Push,
                        "Add Repository…",
                        v.on(Msg::AddRepo),
                    )
                    .default_button(),
                )
                .into(),
        )
    }

    fn list(&self, v: &ViewCx<Msg>) -> TreeList {
        let query = list::normalise(&self.query);
        let cx = list::Context {
            model: &self.model,
            query: &query,
            file_manager: self.file_manager,
        };
        TreeList {
            sections: list::sections(&cx, |id| !self.collapsed.contains(id), v),
            selected: self.selected.clone(),
            on_select: v.map(Msg::Select),
            on_toggle: v.map(|(k, open)| Msg::Toggle(k, open)),
            on_reorder: Some(v.map(|(k, before)| Msg::Reorder(k, before))),
            on_activate: v.map(Msg::Activate),
        }
    }
}

impl Program for Ui {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::ModelChanged => self.model_changed(cx),
            Msg::UpdaterChanged => {}

            Msg::Query(q) => {
                if q == self.query {
                    return;
                }
                let was = self.searching();
                self.query = q;
                let now = self.searching();
                // A search opens every card; clearing it closes again the
                // ones that were closed before it.
                if now {
                    self.collapsed_before_search
                        .get_or_insert_with(|| self.collapsed.clone());
                    self.collapsed.clear();
                } else if was {
                    if let Some(saved) = self.collapsed_before_search.take() {
                        self.collapsed = saved;
                    }
                }
                self.prune_selection();
            }
            // Rendered, though the list already shows it: the menus' enabled
            // items follow the selection.
            Msg::Select(key) => {
                if self.selected != key {
                    self.selected = key;
                    self.remember();
                }
            }
            Msg::Toggle(key, open) => {
                if let Some(Item::Repo { repo_id }) = Item::find(&key, &self.model) {
                    if open {
                        self.collapsed.remove(&repo_id);
                    } else {
                        self.collapsed.insert(repo_id);
                    }
                    self.prune_selection();
                    self.remember();
                }
            }
            Msg::Reorder(key, before) => {
                let Some(Item::Repo { repo_id }) = Item::find(&key, &self.model) else {
                    return;
                };
                let before = before.and_then(|b| match Item::find(&b, &self.model) {
                    Some(Item::Repo { repo_id }) => Some(repo_id),
                    _ => None,
                });
                if let Some(before) = self.repo_move(&repo_id, before.as_deref()) {
                    self.app.dispatch(Action::MoveRepo { repo_id, before });
                }
            }
            // Space opens the selected worktree's branch picker, the way
            // Return opens a row's default action elsewhere.
            Msg::Activate(key) => self.open_picker(&key),

            Msg::AddRepo => {
                self.pick_waiting = true;
                self.pick_repos_if_free(cx);
            }
            Msg::PickedRepos(paths) => {
                if !paths.is_empty() {
                    self.app.dispatch(Action::AddRepos(paths));
                }
            }
            Msg::Refresh => self.app.dispatch(Action::RefreshAll),
            Msg::OpenSettings => {
                if self.settings.is_none() {
                    self.settings = Some(Settings::new(&self.model.config));
                }
                cx.effect(Effect::PresentPanel(settings::KEY.into()));
            }
            Msg::NewWorktree => {
                if let Some(repo_id) = self.creatable_repo_id() {
                    self.open_new_worktree(&repo_id, cx);
                }
            }
            Msg::SwitchBranch => {
                if let Some(key) = self.selected.clone() {
                    self.open_picker(&key);
                }
            }
            Msg::MoveRepo { up } => {
                if let Some((repo_id, before)) = self.selected_repo_move(up) {
                    self.app.dispatch(Action::MoveRepo { repo_id, before });
                    // A card moved from the keyboard stays in view.
                    cx.effect(Effect::RevealSelection);
                }
            }
            Msg::FocusSearch => cx.effect(Effect::FocusSearch),
            Msg::CheckForUpdates => self.updater.check_now(),
            Msg::RestartForUpdate => self.updater.install_or_restart(),

            Msg::DismissNotice => self.app.dispatch(Action::ClearNotice),
            Msg::NoticeDetails => {
                if let Some(n) = &self.model.notice {
                    let title = n
                        .text
                        .lines()
                        .next()
                        .unwrap_or("Details")
                        .trim_end_matches(':')
                        .to_string();
                    let text = n.text.clone();
                    self.ask(Kind::Text { title, text });
                }
            }
            Msg::NoticeExpired(id) => {
                if self.app.model().notice.as_ref().map(|n| n.id) == Some(id) {
                    self.app.dispatch(Action::ClearNotice);
                }
            }

            Msg::NewWorktreeIn(repo_id) => self.open_new_worktree(&repo_id, cx),
            Msg::RepoSettings(repo_id) => {
                if let Some(s) = dialogs::RepoSettings::new(&self.model, &repo_id) {
                    self.ask(Kind::RepoSettings(s));
                }
            }
            Msg::Copy(text) => cx.effect(Effect::Copy(text)),
            Msg::Op { op, repo_id, path } => self.app.dispatch(match op {
                Op::Push => Action::Push { repo_id, path },
                Op::Pull => Action::Pull { repo_id, path },
                Op::PullMain => Action::PullMain { repo_id, path },
                Op::Editor => Action::OpenInEditor(path),
                Op::Terminal => Action::OpenInTerminal(path),
                Op::Reveal => Action::Reveal(path),
            }),
            Msg::Delete { repo_id, path } => {
                if let Some(kind) = dialogs::confirm_delete(&self.model, &repo_id, &path) {
                    self.ask(kind);
                }
            }
            Msg::DeleteDone {
                params,
                reason,
                message,
            } => self.delete_refused(params, reason, message),
            Msg::DismissCreation(id) => self.app.dispatch(Action::DismissCreation(id)),
            Msg::OpenPicker(key) => self.open_picker(&key),
            Msg::Picker(id, m) => self.picker_msg(id, m, cx),

            Msg::DialogField(id, field, value) => {
                if let Some(d) = self.dialogs.iter_mut().find(|d| d.id == id) {
                    d.set_field(field, value);
                }
                self.recheck_sheets(cx);
            }
            Msg::DialogMode(id, mode) => {
                if let Some(Open {
                    kind: Kind::NewWorktree(n),
                    ..
                }) = self.dialogs.iter_mut().find(|d| d.id == id)
                {
                    n.new_branch = mode == 0;
                }
                // A base ref that no longer applies must not hold Create back.
                self.recheck_sheets(cx);
            }
            Msg::DialogDone(id, outcome) => self.dialog_done(id, outcome, cx),
            Msg::DialogGate => self.dialog_gate = false,
            Msg::FolderLooked { path, there } => {
                self.looking.remove(&path);
                let before = self.folders.insert(path, there);
                if before == Some(there) {
                    cx.unchanged();
                    return;
                }
                self.recheck_sheets(cx);
            }

            Msg::Settings(m) => self.settings_msg(m, cx),
        }
        self.pick_repos_if_free(cx);
    }

    fn host(&mut self, event: HostEvent, cx: &mut Cx<Msg>) {
        match event {
            HostEvent::Started { screens } => {
                // Back where it was, unless that frame no longer lands on a
                // screen: the display it was on may be gone, and a window
                // off the edge cannot be dragged back.
                let screens: Vec<WindowFrame> = screens.iter().map(|f| to_core(*f)).collect();
                self.initial_frame = self
                    .frame
                    .filter(|f| screens.is_empty() || f.is_usable_on(&screens))
                    .map(to_toolkit);
                let proxy = cx.proxy();
                let queued = self.model_queued.clone();
                self.app.subscribe(move |_| {
                    if !queued.swap(true, Ordering::AcqRel) {
                        proxy.send(Msg::ModelChanged);
                    }
                });
                let changed = cx.on(Msg::UpdaterChanged);
                self.updater.start(Box::new(move || changed.call(())));
                self.app.start();
                self.last_activation = Some(Instant::now());
                self.model = self.app.model();
                self.try_restore(cx);
                // The list starts focused, so the arrow keys work at once.
                cx.effect(Effect::FocusList);
            }
            // Coming back to the app is the moment stale status would be
            // noticed; a refresh is cheap and runs off the main thread. The
            // launch activation is skipped (start() already lists), as are
            // rapid re-activations.
            HostEvent::Activated => {
                let now = Instant::now();
                let recent = self
                    .last_activation
                    .is_none_or(|t| now.duration_since(t) < ACTIVATION_REFRESH_GAP);
                self.last_activation = Some(now);
                if !recent {
                    self.app.dispatch(Action::RefreshAll);
                }
                cx.unchanged();
            }
            // Last chance to write the window state: the core's delayed
            // write would never run once the process is going away.
            HostEvent::WillQuit => {
                self.remember();
                self.app.flush_ui_state();
                cx.unchanged();
            }
            HostEvent::OpenPaths(paths) => {
                if !paths.is_empty() {
                    self.app.dispatch(Action::AddRepos(paths));
                }
                cx.unchanged();
            }
            HostEvent::FrameChanged(f) => {
                self.frame = Some(to_core(f));
                self.remember();
                cx.unchanged();
            }
            HostEvent::Scrolled(y) => {
                self.scroll = y;
                self.remember();
                cx.unchanged();
            }
        }
    }

    fn view(&self, v: &ViewCx<Msg>) -> View {
        let model = &self.model;
        let enabled = menus::Enabled {
            new_worktree: self.creatable_repo_id().is_some(),
            switch_branch: self.selected_worktree().is_some(),
            move_up: self.selected_repo_move(true).is_some(),
            move_down: self.selected_repo_move(false).is_some(),
            check_for_updates: self.updater.available(),
        };
        View {
            window: MainWindow {
                title: "Worktree Manager".into(),
                min_size: (640.0, 400.0),
                frame: self.initial_frame,
                toolbar: self.toolbar(v),
                notice: self.notice(v),
                empty: self.empty(v),
                list: self.list(v),
                activity: Spinner {
                    spinning: model.refreshing || model.fetching,
                    tooltip: Some(
                        if model.fetching {
                            "Fetching remotes…"
                        } else {
                            "Refreshing…"
                        }
                        .into(),
                    ),
                },
            },
            menus: menus::menus(&enabled, v),
            dialogs: if self.dialog_gate {
                Vec::new()
            } else {
                self.dialogs.iter().map(|d| d.view(v)).collect()
            },
            popover: self.picker.as_ref().map(|p| p.view(v)),
            panels: self
                .settings
                .iter()
                .map(|s| s.view(self.terminal_note, v))
                .collect(),
        }
    }
}

fn to_core(f: Frame) -> WindowFrame {
    WindowFrame {
        x: f.x,
        y: f.y,
        width: f.width,
        height: f.height,
    }
}

fn to_toolkit(f: WindowFrame) -> Frame {
    Frame {
        x: f.x,
        y: f.y,
        width: f.width,
        height: f.height,
    }
}
