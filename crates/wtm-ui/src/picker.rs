//! The branch picker: a filter field over a fuzzy-matched list of a
//! worktree's branches, hung from its branch button. Typing filters and
//! ranks (`wtm_core::fuzzy`), ↑/↓ move, Return chooses, Escape or a click
//! elsewhere closes. Works for detached worktrees too.

use wtm_core::branch_tool::split_tool_prefix;
use wtm_core::fuzzy::{fuzzy_filter, Match};
use wtm_toolkit::{FilterList, Key, ListItem, Popover, Rich, Span, ViewCx, BRANCH_BUTTON};

use crate::list::branch_rich;

#[derive(Debug, Clone)]
pub enum PickerMsg {
    Query(String),
    Move(i32),
    Choose(usize),
    Dismiss,
}

pub struct Picker {
    pub id: u64,
    /// The row it hangs from.
    pub key: Key,
    pub repo_id: String,
    pub path: String,
    all: Vec<String>,
    pub current: Option<String>,
    query: String,
    filtered: Vec<(usize, Match)>,
    selected: Option<usize>,
}

impl Picker {
    pub fn new(
        id: u64,
        key: Key,
        repo_id: String,
        path: String,
        branches: &[String],
        current: Option<String>,
    ) -> Self {
        // The current branch stays choosable even if the list is stale.
        let mut all = Vec::with_capacity(branches.len() + 1);
        if let Some(c) = current.as_ref().filter(|c| !branches.contains(c)) {
            all.push(c.clone());
        }
        all.extend(branches.iter().cloned());
        let mut p = Picker {
            id,
            key,
            repo_id,
            path,
            all,
            current,
            query: String::new(),
            filtered: Vec::new(),
            selected: None,
        };
        p.refilter();
        // Start on the current branch, as a menu would.
        if let Some(row) = p.row_of_current() {
            p.selected = Some(row);
        }
        p
    }

    fn refilter(&mut self) {
        self.filtered = fuzzy_filter(&self.query, &self.all);
        self.selected = (!self.filtered.is_empty()).then_some(0);
    }

    fn row_of_current(&self) -> Option<usize> {
        let current = self.current.as_deref()?;
        self.filtered
            .iter()
            .position(|(i, _)| self.all[*i] == current)
    }

    pub fn set_query(&mut self, query: String) {
        if query != self.query {
            self.query = query;
            self.refilter();
        }
    }

    /// Move the selection by `by` rows, stopping at either end.
    pub fn move_by(&mut self, by: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let last = self.filtered.len() as i64 - 1;
        let from = self.selected.map_or(-1, |s| s as i64);
        self.selected = Some((from + by as i64).clamp(0, last) as usize);
    }

    /// The branch on row `row`, unless it is the one already checked out.
    pub fn choice(&self, row: usize) -> Option<String> {
        let (i, _) = self.filtered.get(row)?;
        let branch = &self.all[*i];
        (self.current.as_deref() != Some(branch.as_str())).then(|| branch.clone())
    }

    /// The selected row, for Return.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn view(&self, v: &ViewCx<crate::Msg>) -> Popover {
        let id = self.id;
        let msg = move |m| crate::Msg::Picker(id, m);
        Popover {
            id,
            anchor: (self.key.clone(), BRANCH_BUTTON),
            list: FilterList {
                query: self.query.clone(),
                placeholder: "e.g., main".into(),
                items: self
                    .filtered
                    .iter()
                    .map(|(i, m)| {
                        let name = &self.all[*i];
                        ListItem {
                            label: highlighted(name, &m.positions),
                            checked: self.current.as_deref() == Some(name.as_str()),
                        }
                    })
                    .collect(),
                selected: self.selected,
                on_query: v.map(move |q| msg(PickerMsg::Query(q))),
                on_move: v.map(move |d| msg(PickerMsg::Move(d))),
                on_choose: v.map(move |r| msg(PickerMsg::Choose(r))),
                on_dismiss: v.on(msg(PickerMsg::Dismiss)),
            },
        }
    }
}

/// `name` with its agent prefix as a mark and the characters at `matched`
/// (indices into the whole name) emphasised.
pub fn highlighted(name: &str, matched: &[usize]) -> Rich {
    let base = branch_rich(name);
    let skip = match split_tool_prefix(name) {
        Some((tool, _)) => tool.prefix().chars().count(),
        None => 0,
    };
    let mut spans: Vec<Span> = base
        .spans
        .iter()
        .filter(|s| matches!(s, Span::Mark(_)))
        .cloned()
        .collect();
    for (i, c) in name.chars().enumerate().skip(skip) {
        let strong = matched.contains(&i);
        match spans.last_mut() {
            Some(Span::Text { text, strong: s }) if *s == strong => text.push(c),
            _ => spans.push(Span::Text {
                text: c.to_string(),
                strong,
            }),
        }
    }
    Rich { spans }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wtm_toolkit::Mark;

    fn picker(branches: &[&str], current: Option<&str>) -> Picker {
        let branches: Vec<String> = branches.iter().map(|s| s.to_string()).collect();
        Picker::new(
            1,
            "w:/p".into(),
            "r".into(),
            "/p".into(),
            &branches,
            current.map(Into::into),
        )
    }

    #[test]
    fn opens_on_the_current_branch_and_keeps_it_when_stale() {
        let p = picker(&["main", "dev"], Some("dev"));
        assert_eq!(p.selected(), Some(1));
        let p = picker(&["main"], Some("gone"));
        assert_eq!(p.all, ["gone", "main"]);
        assert_eq!(p.selected(), Some(0));
    }

    #[test]
    fn choosing_the_current_branch_does_nothing() {
        let p = picker(&["main", "dev"], Some("main"));
        assert_eq!(p.choice(0), None);
        assert_eq!(p.choice(1), Some("dev".into()));
        assert_eq!(p.choice(5), None);
    }

    #[test]
    fn typing_filters_and_selects_the_best_match() {
        let mut p = picker(&["main", "feature/login", "fix"], Some("main"));
        p.set_query("fl".into());
        assert_eq!(p.choice(0), Some("feature/login".into()));
        assert_eq!(p.selected(), Some(0));
        p.move_by(-1);
        assert_eq!(p.selected(), Some(0));
        p.move_by(5);
        assert_eq!(p.selected(), Some(p.filtered.len() - 1));
        p.set_query("zzz".into());
        assert_eq!(p.selected(), None);
    }

    #[test]
    fn matches_are_emphasised_after_the_mark() {
        let r = highlighted("claude/ab", &[7]);
        assert_eq!(
            r.spans,
            vec![
                Span::Mark(Mark::Claude),
                Span::Text {
                    text: "a".into(),
                    strong: true
                },
                Span::Text {
                    text: "b".into(),
                    strong: false
                },
            ]
        );
    }
}
