//! Platform-independent heart of Worktree Manager.
//!
//! - [`types`]: the data model, serialised in the same JSON shape as the
//!   Electron app's store so a config file can be imported unchanged.
//! - [`git`]: a git command runner plus pure, unit-tested porcelain parsers.
//! - [`worktrees`], [`repos`]: orchestration (create/delete safety ladder,
//!   push/pull/switch, adding repositories).
//! - [`config`]: load/save of the configuration file.
//! - [`app`]: the [`App`] facade the UI talks to — synchronous, thread-safe
//!   [`Action`] dispatch, a shared [`Model`] snapshot, and change events.
//!
//! Nothing here knows about a UI toolkit or an operating system; that lives
//! behind the traits in `wtm-platform`.

pub mod app;
pub mod command;
pub mod config;
pub mod fetcher;
pub mod fuzzy;
pub mod git;
pub mod model;
pub mod paths;
pub mod repos;
pub mod types;
pub mod watcher;
pub mod worktrees;

pub use app::{Action, App, Event, Reply};
pub use model::{Busy, Model, PendingCreation, RepoNode};
pub use types::*;
