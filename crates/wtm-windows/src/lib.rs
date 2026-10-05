//! The Windows backend: `wtm_toolkit` on Win32 and Common Controls 6, and
//! the `wtm_platform` services (processes, folders) for Windows.
//!
//! - [`Win32`]: the [`Toolkit`](wtm_toolkit::Toolkit), which owns the
//!   message loop and brings real controls in line with each `View`.
//! - [`WindowsPlatform`] and [`app_dirs`]: launching editors, terminals and
//!   File Explorer, and where the config lives.
//!
//! The executable embeds a manifest (`res/app.manifest`, linked by
//! `build.rs`) asking for Common Controls 6 and per-monitor DPI awareness;
//! without it the controls draw in the Windows 95 style.
//!
//! See `docs/architecture.md` in the repository for how a render reaches
//! the screen. Module by module: `app` (the main window, the state, the
//! message loop's hooks), `list` (the repo cards), `pane` (an element tree
//! as controls), `dialog`, `popover`, `panel`, `menu`, `controls` (making
//! and placing controls), `look` (fonts, colours, GDI and GDI+ drawing).

#![cfg(windows)]

mod app;
mod controls;
mod dialog;
mod list;
mod look;
mod menu;
mod pane;
mod panel;
mod platform;
mod popover;
mod shell;
mod util;

use std::rc::Rc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::{Program, Runtime, Toolkit};

pub use platform::{app_dirs, WindowsPlatform};

/// The Win32 toolkit. `Win32.run(program)` never returns.
pub struct Win32;

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn register(name: PCWSTR, proc_: WndProc, style: WNDCLASS_STYLES) {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style,
        lpfnWndProc: Some(proc_),
        hInstance: controls::instance(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        // Every class paints its own background (`WM_ERASEBKGND`).
        hbrBackground: HBRUSH::default(),
        lpszClassName: name,
        hIcon: unsafe { LoadIconW(Some(controls::instance()), PCWSTR(1 as _)).unwrap_or_default() },
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
    }
}

impl Toolkit for Win32 {
    fn run<P: Program>(self, program: P) -> ! {
        unsafe {
            // The manifest asks for this already; a build without it (see
            // build.rs) still gets sharp text.
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES
                    | ICC_WIN95_CLASSES
                    | ICC_PROGRESS_CLASS
                    | ICC_BAR_CLASSES
                    | ICC_LISTVIEW_CLASSES,
            };
            let _ = InitCommonControlsEx(&icc);
        }
        look::start_gdiplus();
        register(app::MAIN_CLASS, app::main_proc, CS_HREDRAW | CS_VREDRAW);
        register(list::FRAME_CLASS, list::frame_proc, WNDCLASS_STYLES(0));
        // Double clicks open and close a card.
        register(list::CONTENT_CLASS, list::content_proc, CS_DBLCLKS);
        register(
            controls::SPINNER_CLASS,
            controls::spinner_proc,
            WNDCLASS_STYLES(0),
        );
        register(
            controls::PANE_CLASS,
            controls::pane_proc,
            WNDCLASS_STYLES(0),
        );
        register(
            dialog::DIALOG_CLASS,
            dialog::dialog_proc,
            WNDCLASS_STYLES(0),
        );
        register(panel::PANEL_CLASS, panel::panel_proc, WNDCLASS_STYLES(0));
        register(popover::POPOVER_CLASS, popover::popover_proc, CS_DROPSHADOW);

        let main = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                app::MAIN_CLASS,
                windows::core::w!("Worktree Manager"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                None,
                None,
                Some(controls::instance()),
                None,
            )
        };
        let main = match main {
            Ok(h) => h,
            Err(e) => {
                log::error!("could not create the main window: {e}");
                std::process::exit(1);
            }
        };
        look::reset(unsafe { GetDpiForWindow(main) });
        app::install(main);

        let runtime = Runtime::new(program, app::poster(), || {
            Rc::new(app::Backend) as Rc<dyn wtm_toolkit::Backend>
        });
        runtime.start(app::screens());

        let mut msg = MSG::default();
        loop {
            let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if got.0 <= 0 {
                break;
            }
            if app::pre_translate(&msg) {
                continue;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        drop(runtime);
        app::quit()
    }
}
