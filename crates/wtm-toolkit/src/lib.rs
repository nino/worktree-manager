//! The thin wrapper every toolkit backend implements.
//!
//! - [`view`]: the vocabulary — a [`View`] of the whole UI as plain values:
//!   the main window, a sectioned list, stacks of text and buttons, forms,
//!   dialogs, a popover, panels, menus, and one-off [`Effect`]s.
//! - [`runtime`]: the [`Program`] the UI implements, the [`Backend`] a
//!   toolkit implements, and the loop between them that both share.
//! - [`headless`]: a backend without widgets, for driving a program in tests.
//! - [`marks`]: the agent marks as shapes, for toolkits that draw them.
//! - [`layout`]: stack layout, for toolkits that have none of their own.
//!
//! Nothing here knows about a toolkit, an operating system, or the app's
//! model.

pub mod headless;
pub mod layout;
pub mod marks;
pub mod runtime;
pub mod view;

pub use runtime::{
    flush, host_event, Backend, Cx, Poster, Program, Proxy, Runtime, Toolkit, ViewCx,
};
pub use view::*;
