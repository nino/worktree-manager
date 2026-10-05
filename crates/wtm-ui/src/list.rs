//! What the list shows: which repos and worktrees a search leaves, in what
//! order, and what each row says. Pure functions of the model, so the rules
//! are tested here and every toolkit shows the same thing.

use wtm_core::branch_tool::{split_tool_prefix, BranchTool};
use wtm_core::paths::{slugify_branch, tildify};
use wtm_core::{Focus, Model, RepoNode, WorktreeInfo};
use wtm_toolkit::{
    Badge, Emphasis, Hue, Icon, Key, Mark, PendingRow, RepoHeader, Rich, Row, RowAction,
    RowContent, Section, Span, Tint, ViewCx, WorktreeRow,
};

use crate::Msg;

// MARK: Row identity

/// What a row of the list shows. Rows are keyed by identity, never by
/// position: `r:<repo id>`, `w:<worktree path>`, `p:<creation id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Repo { repo_id: String },
    Worktree { repo_id: String, path: String },
    Pending { repo_id: String, id: u64 },
}

impl Item {
    pub fn key(&self) -> Key {
        match self {
            Item::Repo { repo_id } => format!("r:{repo_id}"),
            Item::Worktree { path, .. } => format!("w:{path}"),
            Item::Pending { id, .. } => format!("p:{id}"),
        }
    }

    pub fn repo_id(&self) -> &str {
        match self {
            Item::Repo { repo_id }
            | Item::Worktree { repo_id, .. }
            | Item::Pending { repo_id, .. } => repo_id,
        }
    }

    /// What `key` names in `model`, if it is still there.
    pub fn find(key: &str, model: &Model) -> Option<Item> {
        if let Some(repo_id) = key.strip_prefix("r:") {
            return model.repo(repo_id).map(|_| Item::Repo {
                repo_id: repo_id.to_string(),
            });
        }
        if let Some(path) = key.strip_prefix("w:") {
            return model.repos.iter().find_map(|n| {
                n.worktree(path).map(|_| Item::Worktree {
                    repo_id: n.repo.id.clone(),
                    path: path.to_string(),
                })
            });
        }
        let id: u64 = key.strip_prefix("p:")?.parse().ok()?;
        model
            .pending
            .iter()
            .find(|p| p.id == id)
            .map(|p| Item::Pending {
                repo_id: p.repo_id.clone(),
                id,
            })
    }

    /// The row as the window state remembers it. A pending creation is not
    /// remembered: it is gone by the next launch.
    pub fn focus(&self) -> Option<Focus> {
        match self {
            Item::Repo { repo_id } => Some(Focus {
                repo_id: repo_id.clone(),
                worktree_path: None,
            }),
            Item::Worktree { repo_id, path } => Some(Focus {
                repo_id: repo_id.clone(),
                worktree_path: Some(path.clone()),
            }),
            Item::Pending { .. } => None,
        }
    }

    pub fn from_focus(f: &Focus) -> Item {
        match &f.worktree_path {
            Some(path) => Item::Worktree {
                repo_id: f.repo_id.clone(),
                path: path.clone(),
            },
            None => Item::Repo {
                repo_id: f.repo_id.clone(),
            },
        }
    }
}

// MARK: Search and order

/// The query as matched: trimmed and lower-cased.
pub fn normalise(query: &str) -> String {
    query.trim().to_lowercase()
}

/// Case-insensitive substring match on the branch or the path as the row
/// shows it (`~` for the home folder). Not the full path: every worktree's
/// starts with the home folder, so a query like `users` would match them all.
pub fn matches(query: &str, w: &WorktreeInfo, home: &str) -> bool {
    w.branch
        .as_deref()
        .is_some_and(|b| b.to_lowercase().contains(query))
        || tildify(&w.path, home).to_lowercase().contains(query)
}

/// One card's rows, in the order git will list them: the primary worktree
/// first, then by folder name; a creation sits where its folder will.
pub enum Child<'a> {
    Worktree(&'a WorktreeInfo),
    Pending(&'a wtm_core::PendingCreation),
}

pub fn children<'a>(model: &'a Model, node: &'a RepoNode, query: &str) -> Vec<Child<'a>> {
    let mut rows: Vec<(bool, String, Child)> = Vec::new();
    for w in &node.worktrees {
        if !query.is_empty() && !matches(query, w, &model.home) {
            continue;
        }
        rows.push((
            w.is_main,
            folder_name(&w.path).to_string(),
            Child::Worktree(w),
        ));
    }
    for p in model.pending_for(&node.repo.id) {
        rows.push((false, slugify_branch(&p.branch), Child::Pending(p)));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    rows.into_iter().map(|r| r.2).collect()
}

/// The last component of a worktree's path, as git prints it: always with
/// `/`, on every platform (see `wtm_core::paths`).
fn folder_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}

/// Whether a search shows `node`: one with a matching row, or one whose
/// listing failed (so the error is not hidden by typing).
pub fn shows_repo(model: &Model, node: &RepoNode, query: &str) -> bool {
    query.is_empty() || node.error.is_some() || !children(model, node, query).is_empty()
}

/// The repos the list shows for `query`, in order. Moves are worked out
/// against this, so a repo a search hides never moves with the one dragged.
pub fn shown_repo_ids<'a>(model: &'a Model, query: &str) -> Vec<&'a str> {
    model
        .repos
        .iter()
        .filter(|n| shows_repo(model, n, query))
        .map(|n| n.repo.id.as_str())
        .collect()
}

/// The keys of the rows on screen, in order, given which cards are open.
pub fn visible_keys(model: &Model, query: &str, open: impl Fn(&str) -> bool) -> Vec<Key> {
    let mut out = Vec::new();
    for node in &model.repos {
        if !shows_repo(model, node, query) {
            continue;
        }
        out.push(
            Item::Repo {
                repo_id: node.repo.id.clone(),
            }
            .key(),
        );
        if open(&node.repo.id) {
            for c in children(model, node, query) {
                out.push(child_item(node, &c).key());
            }
        }
    }
    out
}

fn child_item(node: &RepoNode, c: &Child) -> Item {
    match c {
        Child::Worktree(w) => Item::Worktree {
            repo_id: node.repo.id.clone(),
            path: w.path.clone(),
        },
        Child::Pending(p) => Item::Pending {
            repo_id: node.repo.id.clone(),
            id: p.id,
        },
    }
}

// MARK: What each row says

/// Everything the list needs besides the model.
pub struct Context<'a> {
    pub model: &'a Model,
    pub query: &'a str,
    pub file_manager: &'a str,
}

pub fn sections(cx: &Context, open: impl Fn(&str) -> bool, v: &ViewCx<Msg>) -> Vec<Section> {
    let searching = !cx.query.is_empty();
    let mut out = Vec::new();
    for node in &cx.model.repos {
        let rows = children(cx.model, node, cx.query);
        if searching && rows.is_empty() && node.error.is_none() {
            continue;
        }
        let header = repo_header(cx, node, rows.len(), v);
        let rows = rows
            .iter()
            .map(|c| Row {
                key: child_item(node, c).key(),
                content: match c {
                    Child::Worktree(w) => RowContent::Worktree(worktree_row(cx, node, w, v)),
                    Child::Pending(p) => RowContent::Pending(PendingRow {
                        branch: p.branch.clone(),
                        error: p.error.clone(),
                        on_dismiss: v.on(Msg::DismissCreation(p.id)),
                    }),
                },
            })
            .collect();
        out.push(Section {
            key: Item::Repo {
                repo_id: node.repo.id.clone(),
            }
            .key(),
            header,
            // A search opens every card.
            expanded: searching || open(&node.repo.id),
            rows,
        });
    }
    out
}

fn repo_header(cx: &Context, node: &RepoNode, visible: usize, v: &ViewCx<Msg>) -> RepoHeader {
    let total = node.worktrees.len();
    let plural = if total == 1 { "" } else { "s" };
    let count = if cx.query.is_empty() {
        format!("{total} worktree{plural}")
    } else {
        format!("{visible} of {total} worktree{plural}")
    };
    let id = node.repo.id.clone();
    RepoHeader {
        name: node.repo.name.clone(),
        meta: format!("{} · {count}", node.repo.main_branch),
        path: tildify(&node.repo.path, &cx.model.home),
        path_full: node.repo.path.clone(),
        error: node.error.clone(),
        loading: !node.loaded,
        // Nothing can be created where nothing could be listed: git failed
        // in this repo (most often, its folder is gone), and the error beside
        // the path says why.
        can_create: node.error.is_none(),
        on_new_worktree: v.on(Msg::NewWorktreeIn(id.clone())),
        on_settings: v.on(Msg::RepoSettings(id)),
        on_copy_path: v.on(Msg::Copy(node.repo.path.clone())),
    }
}

/// What a worktree's operations act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Push,
    Pull,
    PullMain,
    Editor,
    Terminal,
    Reveal,
}

fn worktree_row(cx: &Context, node: &RepoNode, w: &WorktreeInfo, v: &ViewCx<Msg>) -> WorktreeRow {
    let busy = cx.model.busy_for(&w.path);
    let missing = w.prunable;
    let enabled = busy.is_none() && !missing;
    let shown = w.branch.as_deref().unwrap_or("(detached)");
    let (repo_id, path) = (node.repo.id.clone(), w.path.clone());
    let op = |op: Op| {
        v.on(Msg::Op {
            op,
            repo_id: repo_id.clone(),
            path: path.clone(),
        })
    };
    let action = |id, icon, hint: String, enabled, on_press| RowAction {
        id,
        icon,
        hint,
        enabled,
        hidden: false,
        tint: Tint::Normal,
        on_press,
    };
    WorktreeRow {
        branch: branch_rich(shown),
        branch_name: shown.to_string(),
        can_switch: enabled,
        switch_hint: format!("Switch branch (current: {shown})"),
        on_switch: v.on(Msg::OpenPicker(
            Item::Worktree {
                repo_id: repo_id.clone(),
                path: path.clone(),
            }
            .key(),
        )),
        on_copy_branch: w.branch.clone().map(|b| v.on(Msg::Copy(b))),
        badges: badges_for(w, &node.repo.main_branch),
        busy: busy.map(|b| b.label().to_string()),
        path: tildify(&w.path, &cx.model.home),
        path_full: w.path.clone(),
        on_copy_path: v.on(Msg::Copy(w.path.clone())),
        actions: vec![
            vec![
                action("push", Icon::Push, "Push".into(), enabled, op(Op::Push)),
                action(
                    "pull",
                    Icon::Pull,
                    "Pull (fast-forward only)".into(),
                    enabled,
                    op(Op::Pull),
                ),
                action(
                    "merge",
                    Icon::Merge,
                    format!("Pull {} into this branch", node.repo.main_branch),
                    enabled,
                    op(Op::PullMain),
                ),
            ],
            vec![
                action(
                    "editor",
                    Icon::Editor,
                    "Open in editor".into(),
                    !missing,
                    op(Op::Editor),
                ),
                action(
                    "terminal",
                    Icon::Terminal,
                    "Open in terminal".into(),
                    !missing,
                    op(Op::Terminal),
                ),
                action(
                    "reveal",
                    Icon::Folder,
                    format!("Reveal in {}", cx.file_manager),
                    !missing,
                    op(Op::Reveal),
                ),
            ],
            vec![RowAction {
                // The primary tree cannot be deleted; the core refuses it too.
                hidden: w.is_main,
                tint: Tint::Danger,
                ..action(
                    "delete",
                    Icon::Delete,
                    "Delete worktree".into(),
                    busy.is_none(),
                    v.on(Msg::Delete {
                        repo_id: repo_id.clone(),
                        path: path.clone(),
                    }),
                )
            }],
        ],
    }
}

/// A branch name with a coding agent's prefix drawn as its mark, so the part
/// that names the work is not pushed off the end of a narrow row.
pub fn branch_rich(branch: &str) -> Rich {
    match split_tool_prefix(branch) {
        Some((tool, rest)) => Rich {
            spans: vec![
                Span::Mark(match tool {
                    BranchTool::Claude => Mark::Claude,
                    BranchTool::Cursor => Mark::Cursor,
                }),
                Span::Text {
                    text: rest.to_string(),
                    strong: false,
                },
            ],
        },
        None => Rich::plain(branch),
    }
}

// MARK: Badges

fn badge(
    text: impl Into<String>,
    hue: Hue,
    emphasis: Emphasis,
    tooltip: impl Into<String>,
) -> Badge {
    Badge {
        text: text.into(),
        hue,
        emphasis,
        tooltip: tooltip.into(),
    }
}

/// The badge row for a worktree, in display order. In the light appearance
/// each state has its own hue; where colour is rationed only uncommitted
/// work and a missing folder spend it (`Emphasis`).
pub fn badges_for(w: &WorktreeInfo, main_branch: &str) -> Vec<Badge> {
    use Emphasis::*;
    let mut out = Vec::new();
    if w.is_main {
        out.push(badge(
            "primary",
            Hue::Accent,
            Quiet,
            "The repository's primary working tree",
        ));
    }
    if w.locked {
        out.push(badge("locked", Hue::Gray, Quiet, "This worktree is locked"));
    }
    if w.prunable {
        out.push(badge(
            "folder missing",
            Hue::Red,
            Alarm,
            "Folder was deleted outside the app — git still tracks this worktree. Delete the row to clean up git's bookkeeping.",
        ));
        return out;
    }
    let Some(s) = &w.status else {
        out.push(badge(
            "no status",
            Hue::Gray,
            Quiet,
            "Status could not be determined",
        ));
        return out;
    };
    let trunk = if s.trunk_ref.is_empty() {
        main_branch
    } else {
        &s.trunk_ref
    };
    if !s.is_dirty() {
        out.push(badge(
            "✓",
            Hue::Green,
            Quiet,
            "Clean working tree — no staged, unstaged, or untracked changes",
        ));
    }
    if s.has_staged {
        out.push(badge(
            "staged",
            Hue::Blue,
            Attention,
            "Staged, uncommitted changes",
        ));
    }
    if s.has_unstaged {
        out.push(badge(
            "unstaged",
            Hue::Orange,
            Attention,
            "Unstaged changes",
        ));
    }
    if s.has_untracked {
        out.push(badge(
            "untracked",
            Hue::Purple,
            Attention,
            "Untracked files",
        ));
    }
    match (s.ahead_of_main, s.behind_main) {
        (Some(a), Some(b)) => {
            if a > 0 {
                out.push(badge(
                    format!("↑{a} {trunk}"),
                    Hue::Teal,
                    Quiet,
                    format!("{a} commit(s) ahead of {trunk}"),
                ));
            }
            if b > 0 {
                out.push(badge(
                    format!("↓{b} {trunk}"),
                    Hue::Red,
                    Quiet,
                    format!("{b} commit(s) behind {trunk}"),
                ));
            }
        }
        _ => out.push(badge(
            format!("? {trunk}"),
            Hue::Gray,
            Quiet,
            format!("Couldn't compare with {trunk} — check the repo's main-branch setting"),
        )),
    }
    if s.unpushed {
        out.push(badge(
            format!("⇡{} unpushed", s.unpushed_count),
            Hue::Yellow,
            Notable,
            format!("{} unpushed commit(s)", s.unpushed_count),
        ));
    } else if !s.has_upstream {
        out.push(badge(
            "no upstream",
            Hue::Gray,
            Quiet,
            "No upstream branch configured",
        ));
    }
    out
}

// MARK: Notices

/// What the notice bar shows of a message: its first two lines, with a count
/// of what "Details…" holds. Git's file lists can run to hundreds of lines and
/// must never size the window.
pub fn notice_summary(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 2 {
        return text.to_string();
    }
    format!(
        "{}\n{} … ({} more lines)",
        lines[0].trim_end(),
        lines[1].trim(),
        lines.len() - 2
    )
}

/// A notice too long for the bar, which offers "Details…".
pub fn notice_is_long(text: &str) -> bool {
    text.lines().count() > 3 || text.len() > 240
}

#[cfg(test)]
mod tests {
    use super::*;
    use wtm_core::WorktreeStatus;

    fn worktree(path: &str, branch: Option<&str>) -> WorktreeInfo {
        WorktreeInfo {
            path: path.into(),
            branch: branch.map(Into::into),
            head: String::new(),
            is_main: false,
            locked: false,
            prunable: false,
            status: None,
        }
    }

    #[test]
    fn search_matches_the_branch_and_the_path_as_shown() {
        let home = "/Users/ada";
        let w = worktree("/Users/ada/code/wt/app/fix-login", Some("Fix/Login"));
        assert!(matches("fix/login", &w, home));
        assert!(matches("~/code/wt", &w, home));
        assert!(matches("app/fix", &w, home));
        // The home folder is `~` on the row, so its name finds nothing.
        assert!(!matches("ada", &w, home));
        assert!(!matches("users", &w, home));
        assert!(matches(
            "volumes",
            &worktree("/Volumes/src/app", None),
            home
        ));
    }

    #[test]
    fn summary_keeps_short_notices_and_folds_long_ones() {
        assert_eq!(notice_summary("Added a"), "Added a");
        assert_eq!(notice_summary("a\nb"), "a\nb");
        assert_eq!(
            notice_summary("error: x:\n  f1\n  f2\n  f3"),
            "error: x:\nf1 … (2 more lines)"
        );
    }

    #[test]
    fn agent_prefixes_become_marks() {
        assert_eq!(
            branch_rich("claude/fix").spans,
            vec![
                Span::Mark(Mark::Claude),
                Span::Text {
                    text: "fix".into(),
                    strong: false
                }
            ]
        );
        assert_eq!(branch_rich("claude/fix").to_plain(), "fix");
        assert_eq!(branch_rich("claude"), Rich::plain("claude"));
    }

    #[test]
    fn keys_round_trip_through_focus() {
        let item = Item::Worktree {
            repo_id: "r1".into(),
            path: "/w/a".into(),
        };
        assert_eq!(Item::from_focus(&item.focus().unwrap()), item);
        assert_eq!(item.key(), "w:/w/a");
        assert_eq!(
            Item::Pending {
                repo_id: "r".into(),
                id: 3
            }
            .focus(),
            None
        );
    }

    // Emphasis governs only where colour is rationed; in light every badge
    // has its hue.
    fn clean_status() -> WorktreeStatus {
        WorktreeStatus {
            has_unstaged: false,
            has_staged: false,
            has_untracked: false,
            trunk_ref: "origin/main".into(),
            ahead_of_main: Some(0),
            behind_main: Some(0),
            unpushed: false,
            unpushed_count: 0,
            has_upstream: true,
        }
    }

    fn with_status(status: Option<WorktreeStatus>) -> WorktreeInfo {
        WorktreeInfo {
            status,
            head: "abc1234".into(),
            ..worktree("/w", Some("feature/thing"))
        }
    }

    fn emphases(w: &WorktreeInfo) -> Vec<Emphasis> {
        badges_for(w, "main")
            .into_iter()
            .map(|b| b.emphasis)
            .collect()
    }

    fn coloured(w: &WorktreeInfo) -> usize {
        emphases(w)
            .into_iter()
            .filter(|e| matches!(e, Emphasis::Attention | Emphasis::Alarm))
            .count()
    }

    #[test]
    fn a_clean_in_sync_worktree_spends_no_colour() {
        assert_eq!(coloured(&with_status(Some(clean_status()))), 0);
    }

    #[test]
    fn distance_from_the_trunk_stays_quiet() {
        let mut s = clean_status();
        s.ahead_of_main = Some(4);
        s.behind_main = Some(264);
        assert!(emphases(&with_status(Some(s)))
            .iter()
            .all(|e| *e == Emphasis::Quiet));
    }

    #[test]
    fn unpushed_commits_are_notable_rather_than_coloured() {
        let mut s = clean_status();
        s.unpushed = true;
        s.unpushed_count = 2;
        let w = with_status(Some(s));
        assert!(emphases(&w).contains(&Emphasis::Notable));
        assert_eq!(coloured(&w), 0);
    }

    #[test]
    fn only_uncommitted_work_and_a_missing_folder_are_coloured() {
        let mut s = clean_status();
        s.has_staged = true;
        s.has_unstaged = true;
        s.has_untracked = true;
        assert_eq!(coloured(&with_status(Some(s))), 3);

        let mut gone = with_status(None);
        gone.prunable = true;
        assert_eq!(coloured(&gone), 1);

        // A row the app could not read is a gap in knowledge, not an alarm.
        assert_eq!(coloured(&with_status(None)), 0);
    }

    #[test]
    fn primary_and_locked_markers_are_labels_not_alerts() {
        let mut main = with_status(Some(clean_status()));
        main.is_main = true;
        main.locked = true;
        assert_eq!(coloured(&main), 0);
    }
}
