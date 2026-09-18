//! Persisted configuration: a JSON file in the platform's config directory,
//! byte-compatible with the Electron app's electron-store file so an existing
//! `worktree-manager.json` is imported on first launch.

use std::path::{Path, PathBuf};

use log::{info, warn};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wtm_platform::AppDirs;

use crate::paths::sanitize_repo_name;
use crate::types::{AppConfig, AppSettings, RepoConfig};
use crate::update::UpdateChannel;

const CONFIG_FILE: &str = "config.json";

/// Owns the config file path and the in-memory copy.
#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
}

impl ConfigStore {
    /// Load the config, importing a legacy file if this app has none yet, and
    /// falling back to defaults when nothing is readable.
    pub fn load(dirs: &AppDirs) -> Self {
        let path = dirs.config_dir.join(CONFIG_FILE);
        let config = read_json(&path, "config")
            .or_else(|| {
                dirs.legacy_config_files.iter().find_map(|legacy| {
                    let c = read_json(legacy, "config")?;
                    info!("imported configuration from {}", legacy.display());
                    Some(c)
                })
            })
            .unwrap_or_else(|| defaults(&dirs.home));
        let store = Self { path, config };
        store.save();
        store
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn repo(&self, repo_id: &str) -> Option<&RepoConfig> {
        self.config.repos.iter().find(|r| r.id == repo_id)
    }

    pub fn set_settings(&mut self, settings: AppSettings) {
        self.config.worktrees_root = settings.worktrees_root;
        self.config.editor_command = settings.editor_command;
        self.config.update_channel = settings.update_channel;
        self.save();
    }

    /// Add a repository. Returns the stored (sanitised) display name, or `None`
    /// if a repo with that path already exists.
    pub fn add_repo(&mut self, path: String, main_branch: String) -> Option<String> {
        if self.config.repos.iter().any(|r| r.path == path) {
            return None;
        }
        let base = Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = sanitize_repo_name(&base);
        self.config.repos.push(RepoConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.clone(),
            path,
            main_branch,
            init_command: String::new(),
            commands: Vec::new(),
        });
        self.save();
        Some(name)
    }

    /// Replace a repo's configuration (matched by id). Returns false if unknown.
    pub fn update_repo(&mut self, mut repo: RepoConfig) -> bool {
        repo.name = sanitize_repo_name(&repo.name);
        match self.config.repos.iter_mut().find(|r| r.id == repo.id) {
            Some(slot) => {
                *slot = repo;
                self.save();
                true
            }
            None => false,
        }
    }

    pub fn remove_repo(&mut self, repo_id: &str) {
        self.config.repos.retain(|r| r.id != repo_id);
        self.save();
    }

    fn save(&self) {
        write_json(&self.path, &self.config, "config");
    }
}

/// Read a JSON file, or `None` when there is none or it does not parse (a
/// file from a future version, or one edited by hand).
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path, what: &str) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            warn!("ignoring unreadable {what} {}: {e}", path.display());
            None
        }
    }
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T, what: &str) {
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            warn!("cannot create config dir {}: {e}", dir.display());
            return;
        }
    }
    let json = match serde_json::to_string_pretty(value) {
        Ok(j) => j,
        Err(e) => {
            warn!("cannot serialise {what}: {e}");
            return;
        }
    };
    // Write-then-rename so a crash mid-write never leaves a truncated file.
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, path)) {
        warn!("cannot write {what} {}: {e}", path.display());
    }
}

pub(crate) fn defaults(home: &Path) -> AppConfig {
    AppConfig {
        worktrees_root: home
            .join(".claude-worktrees")
            .to_string_lossy()
            .into_owned(),
        editor_command: "code".to_string(),
        update_channel: UpdateChannel::default(),
        repos: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electron_store_file_round_trips() {
        let json = r#"{"worktreesRoot":"/w","editorCommand":"code","repos":[{"id":"1","name":"a","path":"/a","mainBranch":"main","initCommand":"pnpm i","commands":[{"id":"c","name":"Dev","command":"pnpm dev"}]}]}"#;
        let c: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.repos[0].init_command, "pnpm i");
        assert_eq!(c.repos[0].commands[0].command, "pnpm dev");
        let back = serde_json::to_string(&c).unwrap();
        assert!(back.contains("\"mainBranch\":\"main\""));
    }

    #[test]
    fn missing_optional_fields_default() {
        let c: AppConfig =
            serde_json::from_str(r#"{"worktreesRoot":"/w","editorCommand":"c"}"#).unwrap();
        assert!(c.repos.is_empty());
        // An Electron-era file, and every existing install, follows stable.
        assert_eq!(c.update_channel, UpdateChannel::Stable);
    }

    #[test]
    fn the_update_channel_round_trips() {
        let json = r#"{"worktreesRoot":"/w","editorCommand":"c","updateChannel":"beta"}"#;
        let c: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.update_channel, UpdateChannel::Beta);
        assert!(serde_json::to_string(&c)
            .unwrap()
            .contains(r#""updateChannel":"beta""#));
    }
}
