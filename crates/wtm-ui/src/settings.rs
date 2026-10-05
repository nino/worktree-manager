//! The Settings window. There is no Save or Cancel: every edit applies as it
//! is made, so the window can simply be closed when done. Its fields are
//! drafts seeded when it opens; what is applied is the trimmed draft, and the
//! draft keeps what was typed (a trailing space after `code` survives).

use wtm_core::update::UpdateChannel;
use wtm_core::{AppConfig, AppSettings};
use wtm_toolkit::{Button, ButtonKind, Choice, Field, Form, Panel, Stack, ViewCx};

use crate::Msg;

pub const KEY: &str = "settings";

/// The channels the popup offers, in the order they appear in it.
pub const CHANNELS: [(UpdateChannel, &str); 2] = [
    (UpdateChannel::Stable, "Stable"),
    (UpdateChannel::Beta, "Beta"),
];

#[derive(Debug, Clone)]
pub enum SettingsMsg {
    Root(String),
    Editor(String),
    Channel(usize),
    Browse,
    Picked(Vec<std::path::PathBuf>),
    Closed,
}

pub struct Settings {
    pub root: String,
    pub editor: String,
    pub channel: usize,
}

impl Settings {
    pub fn new(config: &AppConfig) -> Self {
        Settings {
            root: config.worktrees_root.clone(),
            editor: config.editor_command.clone(),
            channel: CHANNELS
                .iter()
                .position(|(c, _)| *c == config.update_channel)
                .unwrap_or(0),
        }
    }

    pub fn channel(&self) -> UpdateChannel {
        CHANNELS
            .get(self.channel)
            .map_or(UpdateChannel::Stable, |c| c.0)
    }

    /// What to apply, if anything: never an empty root (the field is
    /// mid-edit), and nothing the config already says.
    pub fn changed(&self, config: &AppConfig) -> Option<AppSettings> {
        let settings = AppSettings {
            worktrees_root: self.root.trim().to_string(),
            editor_command: self.editor.trim().to_string(),
            update_channel: self.channel(),
        };
        if settings.worktrees_root.is_empty() {
            return None;
        }
        (config.worktrees_root != settings.worktrees_root
            || config.editor_command != settings.editor_command
            || config.update_channel != settings.update_channel)
            .then_some(settings)
    }

    pub fn view(&self, terminal_note: &str, v: &ViewCx<Msg>) -> Panel {
        let msg = |m: fn(String) -> SettingsMsg| v.map(move |s| Msg::Settings(m(s)));
        let body = Form::default()
            .row(
                "Worktrees root:",
                Stack::row(6.0)
                    .child(
                        Stack::row(0.0).grow().child(Field {
                            id: "root",
                            value: self.root.clone(),
                            placeholder: "e.g., ~/.claude-worktrees".into(),
                            enabled: true,
                            on_change: msg(SettingsMsg::Root),
                        }),
                    )
                    .child(Button::new(
                        "browse",
                        ButtonKind::Push,
                        "Browse…",
                        v.on(Msg::Settings(SettingsMsg::Browse)),
                    )),
            )
            .hint("Worktrees are created under this folder, grouped by repo name.")
            .row(
                "Editor command:",
                Field {
                    id: "editor",
                    value: self.editor.clone(),
                    placeholder: "e.g., code".into(),
                    enabled: true,
                    on_change: msg(SettingsMsg::Editor),
                },
            )
            .hint("Used by “Open in editor”. The worktree path is appended, or substituted for {path} if present.")
            .hint(terminal_note)
            .row(
                "Software updates:",
                Choice {
                    id: "channel",
                    options: CHANNELS.iter().map(|(_, t)| t.to_string()).collect(),
                    selected: self.channel,
                    // The caption beside it is a plain label, so the popup
                    // would otherwise announce only the channel name.
                    a11y_label: Some("Software updates".into()),
                    on_select: v.map(|i| Msg::Settings(SettingsMsg::Channel(i))),
                },
            )
            .hint("Beta builds arrive before they are released to everyone, and are less tested. Only an app installed from a release updates itself.");
        Panel {
            key: KEY.into(),
            title: "Settings".into(),
            body: body.into(),
            focus: Some("root"),
            on_close: v.on(Msg::Settings(SettingsMsg::Closed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        AppConfig {
            worktrees_root: "/wt".into(),
            editor_command: "code".into(),
            update_channel: UpdateChannel::Stable,
            repos: Vec::new(),
        }
    }

    #[test]
    fn applies_the_trimmed_draft_and_never_an_empty_root() {
        let c = config();
        let mut s = Settings::new(&c);
        assert_eq!(s.changed(&c), None);
        s.editor = "code ".into();
        assert_eq!(s.changed(&c), None, "trailing space is not a change");
        s.editor = "code -w".into();
        assert_eq!(s.changed(&c).unwrap().editor_command, "code -w");
        s.root = "  ".into();
        assert_eq!(s.changed(&c), None);
    }
}
