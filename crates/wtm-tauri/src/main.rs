// A release build on Windows is a GUI program, with no console window of its
// own; a debug build keeps the console for the log.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    wtm_tauri::run();
}
