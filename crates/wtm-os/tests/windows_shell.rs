//! The crate is empty off Windows (`#![cfg(windows)]`), but its quoting,
//! command lines and directories are plain functions: test them everywhere.

#[allow(dead_code)]
#[path = "../src/windows_shell.rs"]
mod shell;
