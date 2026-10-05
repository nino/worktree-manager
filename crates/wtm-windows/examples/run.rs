//! The app on the Win32 backend, without `wtm-app`'s wiring:
//! `cargo run -p wtm-windows --example run` on Windows. Point
//! `WTM_USER_DATA` at a throwaway directory to keep the real config out of
//! it.
//!
//! This is a console program on purpose. `wtm-core` starts git without
//! `CREATE_NO_WINDOW`, so from a GUI-subsystem program every git run would
//! open a console window of its own, which flashes up and takes the
//! keyboard from the app. Sharing this console avoids that, and the log
//! goes to it.

#[cfg(windows)]
fn main() {
    use std::rc::Rc;
    use std::sync::Arc;

    use wtm_windows::{app_dirs, Win32, WindowsPlatform};

    env_logger::init();
    let app = wtm_core::App::new(Arc::new(WindowsPlatform), app_dirs());
    wtm_ui::run(Win32, app, Rc::new(wtm_platform::NoUpdater));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This example runs on Windows only.");
}
