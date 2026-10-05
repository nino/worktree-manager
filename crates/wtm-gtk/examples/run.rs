//! The app on GTK, until `wtm-app` picks this backend on Linux:
//!
//! ```sh
//! WTM_USER_DATA=/tmp/wtm-sandbox cargo run -p wtm-gtk --example run
//! ```

#[cfg(target_os = "linux")]
fn main() {
    use std::rc::Rc;
    use std::sync::Arc;

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let app = wtm_core::App::new(Arc::new(wtm_gtk::LinuxPlatform), wtm_gtk::app_dirs());
    wtm_ui::run(wtm_gtk::Gtk, app, Rc::new(wtm_platform::NoUpdater));
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the GTK backend is built on Linux only");
}
