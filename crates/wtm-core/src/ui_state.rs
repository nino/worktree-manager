//! What the window looked like last time: its frame, how far the list was
//! scrolled, which row had the keyboard, and which cards were closed.
//!
//! This sits beside `config.json` and `snapshot.json` rather than in the
//! platform's own window-restoration store, for two reasons: `WTM_USER_DATA`
//! then sandboxes it along with everything else, so a dev run cannot disturb
//! the real window; and the values are plain numbers and ids, so a backend on
//! another OS gets the behaviour without inventing its own file.
//!
//! Row identity, not row number: the tree is rebuilt from a fresh listing at
//! every launch, and a repo added or a worktree deleted in between would make
//! an index point at something else.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wtm_platform::AppDirs;

use crate::config::{read_json, write_json};
use crate::types::AppConfig;

const UI_STATE_FILE: &str = "ui-state.json";

/// A window's frame in screen coordinates, as the window manager reports it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Fraction of a saved frame that has to fall on a screen for it to be
/// restored: enough of the window — and so of the title bar it is dragged by —
/// to be seen and moved.
const MIN_VISIBLE_FRACTION: f64 = 0.5;

impl WindowFrame {
    fn area(self) -> f64 {
        self.width * self.height
    }

    /// Area shared with `other`.
    fn overlap(self, other: Self) -> f64 {
        let w = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let h = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        if w <= 0.0 || h <= 0.0 {
            0.0
        } else {
            w * h
        }
    }

    /// Whether this frame can still be restored onto `screens` (each one's
    /// visible frame). A window saved on a display that has since been
    /// unplugged, or on a screen that has shrunk, would otherwise come back
    /// somewhere it cannot be reached. Screens do not overlap each other, so
    /// summing lets a window that straddled two of them come back straddling.
    pub fn is_usable_on(self, screens: &[WindowFrame]) -> bool {
        if !self.is_sane() {
            return false;
        }
        let visible: f64 = screens.iter().map(|s| self.overlap(*s)).sum();
        visible >= self.area() * MIN_VISIBLE_FRACTION
    }

    /// Rejects a hand-edited or truncated file: a zero or infinite frame
    /// would otherwise be handed to the window manager.
    fn is_sane(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width >= 1.0
            && self.height >= 1.0
    }
}

/// The row that had the keyboard. A creation still in flight is deliberately
/// not recorded: it is not there at the next launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    /// The repo whose own row, or whose worktree's row, was focused.
    pub repo_id: String,
    /// The worktree's path; `None` when the repo header itself was focused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
}

/// Everything the window comes back to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiState {
    /// `None` until a window has been placed once; the window centres itself.
    pub window: Option<WindowFrame>,
    /// How far the list was scrolled from the top, in points.
    pub scroll: f64,
    pub focus: Option<Focus>,
    /// Repos whose cards were closed. A set, so the same cards closed in a
    /// different order compare equal and do not rewrite the file.
    pub collapsed_repos: BTreeSet<String>,
}

impl UiState {
    /// Forget what the configuration no longer has. Without this the
    /// collapsed list grows for ever as repos come and go, and a focus can
    /// name a repo that is not there to be focused.
    ///
    /// Only repo ids are checked. A worktree path is left alone because it
    /// may simply not be listed yet, and restoring one that has since been
    /// deleted finds no row and does nothing.
    pub fn prune(&mut self, config: &AppConfig) {
        let known = |id: &str| config.repos.iter().any(|r| r.id == id);
        self.collapsed_repos.retain(|id| known(id));
        if self.focus.as_ref().is_some_and(|f| !known(&f.repo_id)) {
            self.focus = None;
        }
    }
}

/// Owns the file and the last value written to it.
#[derive(Debug, Clone)]
pub struct UiStateStore {
    path: PathBuf,
    state: UiState,
}

impl UiStateStore {
    /// Read the saved state, falling back to defaults when there is nothing
    /// readable — a first launch, or a file from a future version.
    pub fn load(dirs: &AppDirs) -> Self {
        let path = dirs.config_dir.join(UI_STATE_FILE);
        let state = read_json(&path, "UI state").unwrap_or_default();
        Self { path, state }
    }

    pub fn state(&self) -> &UiState {
        &self.state
    }

    /// Replace the pending value. Returns whether it differs from what is
    /// already held, so a caller can skip a write that would change nothing.
    pub fn set(&mut self, state: UiState) -> bool {
        if self.state == state {
            return false;
        }
        self.state = state;
        true
    }

    pub fn save(&self) {
        write_json(&self.path, &self.state, "UI state");
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::defaults;
    use crate::types::RepoConfig;

    fn frame(x: f64, y: f64, width: f64, height: f64) -> WindowFrame {
        WindowFrame {
            x,
            y,
            width,
            height,
        }
    }

    fn config(repo_ids: &[&str]) -> AppConfig {
        let mut config = defaults(Path::new("/h"));
        config.repos = repo_ids
            .iter()
            .map(|id| RepoConfig {
                id: (*id).to_string(),
                name: (*id).to_string(),
                path: format!("/{id}"),
                main_branch: "main".into(),
                init_command: String::new(),
                commands: Vec::new(),
            })
            .collect();
        config
    }

    fn ids(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    #[test]
    fn a_missing_file_leaves_everything_at_its_default() {
        let s: UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(s, UiState::default());
        assert!(s.window.is_none());
        assert_eq!(s.scroll, 0.0);
    }

    #[test]
    fn the_state_round_trips_in_camel_case() {
        let s = UiState {
            window: Some(frame(10.0, 20.0, 1000.0, 700.0)),
            scroll: 42.5,
            focus: Some(Focus {
                repo_id: "r1".into(),
                worktree_path: Some("/w/a".into()),
            }),
            collapsed_repos: ids(&["r2"]),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""collapsedRepos":["r2"]"#), "{json}");
        assert!(json.contains(r#""worktreePath":"/w/a""#), "{json}");
        assert_eq!(serde_json::from_str::<UiState>(&json).unwrap(), s);
    }

    #[test]
    fn a_repo_header_focus_omits_the_worktree_path() {
        let s = UiState {
            focus: Some(Focus {
                repo_id: "r1".into(),
                worktree_path: None,
            }),
            ..UiState::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("worktreePath"), "{json}");
        assert_eq!(serde_json::from_str::<UiState>(&json).unwrap(), s);
    }

    #[test]
    fn a_frame_on_a_screen_is_restored() {
        let screens = [frame(0.0, 0.0, 1440.0, 875.0)];
        assert!(frame(100.0, 100.0, 1000.0, 700.0).is_usable_on(&screens));
        // Flush against the edges, and the whole screen, still count.
        assert!(frame(0.0, 0.0, 1440.0, 875.0).is_usable_on(&screens));
        assert!(frame(440.0, 175.0, 1000.0, 700.0).is_usable_on(&screens));
    }

    #[test]
    fn a_frame_off_the_screens_is_not() {
        let screens = [frame(0.0, 0.0, 1440.0, 875.0)];
        // The second display it was on has been unplugged.
        assert!(!frame(1600.0, 200.0, 1000.0, 700.0).is_usable_on(&screens));
        // Mostly past the right edge: the title bar would be unreachable.
        assert!(!frame(1200.0, 100.0, 1000.0, 700.0).is_usable_on(&screens));
        // Dragged mostly below the bottom.
        assert!(!frame(100.0, -600.0, 1000.0, 700.0).is_usable_on(&screens));
        assert!(!frame(100.0, 100.0, 1000.0, 700.0).is_usable_on(&[]));
    }

    #[test]
    fn a_frame_straddling_two_screens_is_restored() {
        // Half on each: neither screen holds enough on its own, together
        // they hold all of it.
        let screens = [
            frame(0.0, 0.0, 1440.0, 875.0),
            frame(1440.0, 0.0, 1440.0, 875.0),
        ];
        assert!(frame(940.0, 100.0, 1000.0, 700.0).is_usable_on(&screens));
    }

    #[test]
    fn a_nonsense_frame_is_rejected() {
        let screens = [frame(0.0, 0.0, 1440.0, 875.0)];
        assert!(!frame(0.0, 0.0, 0.0, 0.0).is_usable_on(&screens));
        assert!(!frame(0.0, 0.0, f64::NAN, 700.0).is_usable_on(&screens));
        assert!(!frame(0.0, 0.0, f64::INFINITY, 700.0).is_usable_on(&screens));
    }

    #[test]
    fn pruning_drops_repos_the_config_no_longer_has() {
        let mut s = UiState {
            focus: Some(Focus {
                repo_id: "gone".into(),
                worktree_path: Some("/w/a".into()),
            }),
            collapsed_repos: ids(&["r2", "gone", "r1"]),
            ..UiState::default()
        };
        s.prune(&config(&["r1", "r2"]));
        assert_eq!(s.collapsed_repos, ids(&["r1", "r2"]));
        assert!(s.focus.is_none());
    }

    #[test]
    fn the_store_writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("wtm-ui-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dirs = AppDirs {
            config_dir: dir.join("config"),
            home: dir.clone(),
            legacy_config_files: Vec::new(),
        };

        let mut store = UiStateStore::load(&dirs);
        assert_eq!(store.state(), &UiState::default());

        let state = UiState {
            window: Some(frame(10.0, 20.0, 1000.0, 700.0)),
            scroll: 120.0,
            focus: Some(Focus {
                repo_id: "r1".into(),
                worktree_path: None,
            }),
            collapsed_repos: ids(&["r2"]),
        };
        assert!(store.set(state.clone()));
        // The same value again is not a change, so nothing is rewritten.
        assert!(!store.set(state.clone()));
        store.save();

        assert_eq!(UiStateStore::load(&dirs).state(), &state);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
