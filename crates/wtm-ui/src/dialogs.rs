//! The modal questions: New Worktree, repo settings, the delete ladder,
//! errors and long notices. Each open one keeps its own drafts of its
//! fields, seeded when it opens; the config is never written back into a
//! field while it is open.
//!
//! Every button carries what it acts on as it was shown: Create sends the
//! source its note named, Delete the branch its question named. A listing
//! that lands between the render and the click cannot change what the click
//! does.

use wtm_core::new_worktree::{self, Check};
use wtm_core::{BranchLocation, CreateWorktreeParams, DeleteWorktreeParams, Model, RepoConfig};
use wtm_toolkit::{
    Combo, Dialog, DialogButton, DialogStyle, Element, Field, Form, Id, Ink, Role, Segmented, Text,
    TextStyle, ViewCx,
};

use crate::Msg;

/// What a dialog's button does once the dialog has closed.
#[derive(Debug, Clone)]
pub enum Outcome {
    Close,
    Create(CreateWorktreeParams),
    SaveRepo(RepoConfig),
    /// Ask whether to remove the repo.
    AskRemove {
        repo_id: String,
        name: String,
    },
    Remove(String),
    Delete(DeleteWorktreeParams),
}

pub enum Kind {
    /// A long message in a scrolling, selectable block.
    Text {
        title: String,
        text: String,
    },
    Error {
        title: String,
        message: String,
    },
    NewWorktree(NewWorktree),
    RepoSettings(RepoSettings),
    ConfirmRemove {
        repo_id: String,
        name: String,
    },
    /// The first rung of the delete ladder.
    ConfirmDelete {
        title: String,
        info: String,
        params: DeleteWorktreeParams,
    },
    /// git refused: the tree has changes. Ask again, louder.
    ForceDelete {
        title: String,
        params: DeleteWorktreeParams,
    },
}

pub struct Open {
    pub id: u64,
    pub kind: Kind,
}

// MARK: New Worktree

pub struct NewWorktree {
    pub repo_id: String,
    repo_name: String,
    pub name: String,
    pub new_branch: bool,
    pub base: String,
    base_options: Vec<String>,
    default_base: String,
    pub check: Check,
}

impl NewWorktree {
    pub fn new(model: &Model, repo_id: &str) -> Option<Self> {
        let node = model.repo(repo_id)?;
        Some(NewWorktree {
            repo_id: repo_id.to_string(),
            repo_name: node.repo.name.clone(),
            name: String::new(),
            new_branch: true,
            base: node.default_base_ref.clone(),
            base_options: node.base_ref_candidates.clone(),
            default_base: node.default_base_ref.clone(),
            check: new_worktree::check("", true, "", &BranchLocation::Unknown, None),
        })
    }

    /// Where a worktree for the typed name would go.
    pub fn target(&self, model: &Model) -> Option<String> {
        let node = model.repo(&self.repo_id)?;
        let path = model.worktree_path(&node.repo, &self.name);
        Some(path.to_string_lossy().into_owned())
    }

    /// Check the fields again: against the latest listing, and `taken`, the
    /// folder for the name as shown when something is already there.
    pub fn recheck(&mut self, model: &Model, taken: Option<&str>) {
        let place = model
            .repo(&self.repo_id)
            .map(|n| n.locate_branch(self.name.trim()))
            .unwrap_or(BranchLocation::Nowhere);
        if let Some(node) = model.repo(&self.repo_id) {
            self.base_options = node.base_ref_candidates.clone();
        }
        self.check = new_worktree::check(&self.name, self.new_branch, &self.base, &place, taken);
    }

    fn view(&self, id: u64, v: &ViewCx<Msg>) -> Dialog {
        let field = move |f: Id| v.map(move |s| Msg::DialogField(id, f, s));
        // Why git would refuse the name, checked as it is typed, with Create
        // off until there is one it would take. It also says when the branch
        // will come from a remote. The line is there even when empty: an
        // alert does not grow once it is on screen.
        let (note, ink) = match &self.check.create {
            Err(reason) => (reason.clone(), Ink::Error),
            Ok(_) => (self.check.note.clone().unwrap_or_default(), Ink::Secondary),
        };
        let body = Form::default()
            .row(
                "Branch name:",
                Field {
                    id: "branch",
                    value: self.name.clone(),
                    placeholder: "e.g., feature/my-thing".into(),
                    enabled: true,
                    on_change: field("branch"),
                },
            )
            .row(
                "",
                Text::new(note)
                    .id("note")
                    .style(TextStyle::Caption)
                    .ink(ink)
                    .lines(1),
            )
            .row(
                "",
                Segmented {
                    id: "mode",
                    options: vec!["New branch".into(), "Existing branch".into()],
                    selected: if self.new_branch { 0 } else { 1 },
                    on_select: v.map(move |i| Msg::DialogMode(id, i)),
                },
            )
            .row_hidden(
                "Base ref:",
                Combo {
                    id: "base",
                    value: self.base.clone(),
                    options: self.base_options.clone(),
                    placeholder: format!("e.g., {}", self.default_base),
                    // A branch from a remote has no base.
                    enabled: !self.check.from_remote,
                    on_change: field("base"),
                },
                !self.new_branch,
            );
        let create = match &self.check.create {
            Ok(source) => Some(CreateWorktreeParams {
                repo_id: self.repo_id.clone(),
                branch: self.name.trim().to_string(),
                source: source.clone(),
            }),
            Err(_) => None,
        };
        Dialog {
            id,
            title: format!("New worktree — {}", self.repo_name),
            message: String::new(),
            style: DialogStyle::Info,
            body: Some(body.into()),
            buttons: vec![
                DialogButton {
                    label: "Create".into(),
                    role: Role::Default,
                    enabled: create.is_some(),
                    on_press: match create {
                        Some(p) => v.on(Msg::DialogDone(id, Outcome::Create(p))),
                        None => v.on(Msg::DialogDone(id, Outcome::Close)),
                    },
                },
                cancel(id, v),
            ],
            focus: Some("branch"),
        }
    }
}

// MARK: Repo settings

pub struct RepoSettings {
    repo: RepoConfig,
    shown_path: String,
    pub name: String,
    pub main: String,
    pub init: String,
}

impl RepoSettings {
    pub fn new(model: &Model, repo_id: &str) -> Option<Self> {
        let repo = model.repo(repo_id)?.repo.clone();
        Some(RepoSettings {
            shown_path: wtm_core::paths::tildify(&repo.path, &model.home),
            name: repo.name.clone(),
            main: repo.main_branch.clone(),
            init: repo.init_command.clone(),
            repo,
        })
    }

    /// The repo as Save would leave it.
    fn saved(&self) -> RepoConfig {
        let mut updated = self.repo.clone();
        updated.name = match self.name.trim() {
            "" => self.repo.name.clone(),
            n => n.to_string(),
        };
        updated.main_branch = match self.main.trim() {
            "" => "main".into(),
            m => m.to_string(),
        };
        updated.init_command = self.init.clone();
        updated
    }

    fn view(&self, id: u64, v: &ViewCx<Msg>) -> Dialog {
        let field = move |f: Id| v.map(move |s| Msg::DialogField(id, f, s));
        let text = |f: Id, value: &str, placeholder: &str| Field {
            id: f,
            value: value.to_string(),
            placeholder: placeholder.into(),
            enabled: true,
            on_change: field(f),
        };
        let body = Form::default()
            .row("Display name:", text("name", &self.name, "e.g., my-app"))
            .row("Main branch:", text("main", &self.main, "e.g., main"))
            .hint("Worktrees show ahead/behind counts relative to this branch.")
            .row("Init command:", text("init", &self.init, "e.g., pnpm i"))
            .hint("Runs inside each new worktree after it is created.");
        Dialog {
            id,
            title: format!("Repo settings — {}", self.repo.name),
            message: self.shown_path.clone(),
            style: DialogStyle::Info,
            body: Some(body.into()),
            buttons: vec![
                DialogButton {
                    label: "Save".into(),
                    role: Role::Default,
                    enabled: true,
                    on_press: v.on(Msg::DialogDone(id, Outcome::SaveRepo(self.saved()))),
                },
                cancel(id, v),
                DialogButton {
                    label: "Remove Repo…".into(),
                    role: Role::Normal,
                    enabled: true,
                    on_press: v.on(Msg::DialogDone(
                        id,
                        Outcome::AskRemove {
                            repo_id: self.repo.id.clone(),
                            name: self.repo.name.clone(),
                        },
                    )),
                },
            ],
            focus: Some("name"),
        }
    }
}

// MARK: Delete

/// The first question of the delete ladder, for a worktree as its row shows
/// it, or `None` when it is not listed any more.
pub fn confirm_delete(model: &Model, repo_id: &str, path: &str) -> Option<Kind> {
    let w = model.repo(repo_id)?.worktree(path)?;
    let shown = w.branch.clone().unwrap_or_else(|| "(detached)".into());
    let mut info = String::new();
    if w.prunable {
        info.push_str("The folder is already gone — this cleans up git's bookkeeping. ");
    }
    match &w.status {
        Some(s) if s.is_dirty() => info.push_str("It has uncommitted changes. "),
        None if !w.prunable => info.push_str("Its status could not be determined. "),
        _ => {}
    }
    info.push_str(&wtm_core::paths::tildify(path, &model.home));
    Some(Kind::ConfirmDelete {
        title: format!("Delete worktree “{shown}”?"),
        info,
        params: DeleteWorktreeParams {
            repo_id: repo_id.to_string(),
            worktree_path: path.to_string(),
            expected_branch: w.branch.clone(),
            force: false,
        },
    })
}

pub fn force_delete(params: DeleteWorktreeParams) -> Kind {
    let shown = params
        .expected_branch
        .clone()
        .unwrap_or_else(|| "(detached)".into());
    Kind::ForceDelete {
        title: format!("“{shown}” has uncommitted changes"),
        params: DeleteWorktreeParams {
            force: true,
            ..params
        },
    }
}

// MARK: Rendering

fn cancel(id: u64, v: &ViewCx<Msg>) -> DialogButton {
    DialogButton {
        label: "Cancel".into(),
        role: Role::Cancel,
        enabled: true,
        on_press: v.on(Msg::DialogDone(id, Outcome::Close)),
    }
}

fn ok(id: u64, v: &ViewCx<Msg>) -> DialogButton {
    DialogButton {
        label: "OK".into(),
        role: Role::Default,
        enabled: true,
        on_press: v.on(Msg::DialogDone(id, Outcome::Close)),
    }
}

fn simple(id: u64, title: &str, message: &str, style: DialogStyle) -> Dialog {
    Dialog {
        id,
        title: title.to_string(),
        message: message.to_string(),
        style,
        body: None,
        buttons: Vec::new(),
        focus: None,
    }
}

impl Open {
    pub fn view(&self, v: &ViewCx<Msg>) -> Dialog {
        let id = self.id;
        let destructive = |label: &str, outcome: Outcome| DialogButton {
            label: label.into(),
            role: Role::Destructive,
            enabled: true,
            on_press: v.on(Msg::DialogDone(id, outcome)),
        };
        match &self.kind {
            Kind::Text { title, text } => Dialog {
                body: Some(Element::TextBlock(text.clone())),
                buttons: vec![ok(id, v)],
                ..simple(id, title, "", DialogStyle::Info)
            },
            Kind::Error { title, message } => Dialog {
                buttons: vec![ok(id, v)],
                ..simple(id, title, message, DialogStyle::Warning)
            },
            Kind::NewWorktree(n) => n.view(id, v),
            Kind::RepoSettings(r) => r.view(id, v),
            Kind::ConfirmRemove { repo_id, name } => Dialog {
                buttons: vec![
                    destructive("Remove", Outcome::Remove(repo_id.clone())),
                    cancel(id, v),
                ],
                ..simple(
                    id,
                    &format!("Remove “{name}” from the list?"),
                    "This does not touch any files on disk.",
                    DialogStyle::Info,
                )
            },
            Kind::ConfirmDelete {
                title,
                info,
                params,
            } => Dialog {
                buttons: vec![
                    destructive("Delete", Outcome::Delete(params.clone())),
                    cancel(id, v),
                ],
                ..simple(id, title, info, DialogStyle::Info)
            },
            Kind::ForceDelete { title, params } => Dialog {
                buttons: vec![
                    destructive(
                        "Force Delete — Discard Changes",
                        Outcome::Delete(params.clone()),
                    ),
                    cancel(id, v),
                ],
                ..simple(
                    id,
                    title,
                    "They will be permanently lost. Delete anyway?",
                    DialogStyle::Critical,
                )
            },
        }
    }

    /// A field of this dialog was edited.
    pub fn set_field(&mut self, field: Id, value: String) {
        match (&mut self.kind, field) {
            (Kind::NewWorktree(n), "branch") => n.name = value,
            (Kind::NewWorktree(n), "base") => n.base = value,
            (Kind::RepoSettings(r), "name") => r.name = value,
            (Kind::RepoSettings(r), "main") => r.main = value,
            (Kind::RepoSettings(r), "init") => r.init = value,
            _ => {}
        }
    }
}
