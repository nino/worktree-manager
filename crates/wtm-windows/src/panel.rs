//! Secondary windows (Settings): a fixed-size window holding a pane.

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::Key;

use crate::app::{with_state, Reg};
use crate::controls::*;
use crate::look::{self, Paint};
use crate::pane::Pane;
use crate::util::*;

pub const PANEL_CLASS: windows::core::PCWSTR = w!("WtmPanel");

// In DIPs.
const WIDTH: f64 = 560.0;
const PAD: f64 = 20.0;

pub struct Panel {
    pub key: Key,
    pub hwnd: HWND,
    tip: HWND,
    pane: Pane,
    size: (i32, i32),
}

impl Panel {
    pub fn open(owner: HWND, p: &wtm_toolkit::Panel, reg: &mut Reg) -> Panel {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT | WS_EX_DLGMODALFRAME,
                PANEL_CLASS,
                &HSTRING::from(p.title.as_str()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                100,
                100,
                Some(owner),
                None,
                Some(instance()),
                None,
            )
            .unwrap_or_default()
        };
        let pal = look::palette();
        look::set_paint(
            key(hwnd),
            Paint::Plain {
                ground: pal.window,
                ink: pal.text,
            },
        );
        let tip = tooltip(hwnd);
        let pane = Pane::new(hwnd, tip, pal.window);
        let mut panel = Panel {
            key: p.key.clone(),
            hwnd,
            tip,
            pane,
            size: (0, 0),
        };
        panel.patch(p, reg);
        // Centred over the main window, like a sheet's window would be.
        let mut o = RECT::default();
        unsafe {
            let _ = GetWindowRect(owner, &mut o);
        }
        let (w, h) = panel.size;
        let work = crate::app::work_area(owner);
        let x =
            (o.left + (o.right - o.left - w) / 2).clamp(work.left, (work.right - w).max(work.left));
        let y =
            (o.top + (o.bottom - o.top - h) / 4).clamp(work.top, (work.bottom - h).max(work.top));
        unsafe {
            let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        panel.focus_first(p);
        panel
    }

    fn focus_first(&self, p: &wtm_toolkit::Panel) {
        let target = p.focus.and_then(|id| self.pane.control(id));
        if let Some(h) = target {
            unsafe {
                let _ = SetFocus(Some(h));
            }
            send(h, EM_SETSEL, 0, -1);
        }
    }

    /// The fonts and colours changed (DPI, system colours): new controls
    /// in the same window, which stays where the user put it.
    pub fn restyle(&mut self, p: &wtm_toolkit::Panel, reg: &mut Reg) {
        let pal = look::palette();
        look::set_paint(
            key(self.hwnd),
            Paint::Plain {
                ground: pal.window,
                ink: pal.text,
            },
        );
        let had_focus = has_focus(self.hwnd);
        let old = std::mem::replace(&mut self.pane, Pane::new(self.hwnd, self.tip, pal.window));
        old.destroy(reg);
        self.size = (0, 0);
        self.patch(p, reg);
        invalidate(self.hwnd);
        if had_focus {
            self.focus_first(p);
        }
    }

    pub fn patch(&mut self, p: &wtm_toolkit::Panel, reg: &mut Reg) {
        set_text_if(self.hwnd, &p.title);
        self.pane.render(&p.body, reg);
        let pad = look::px(PAD);
        let width = look::px(WIDTH);
        let h = self.pane.measure(width as f64).height.ceil() as i32;
        self.pane.place(rect(pad, pad, width, h));
        let mut r = rect(0, 0, width + 2 * pad, h + 2 * pad);
        unsafe {
            let style = WINDOW_STYLE(GetWindowLongW(self.hwnd, GWL_STYLE) as u32);
            let ex = WINDOW_EX_STYLE(GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32);
            let _ = AdjustWindowRectEx(&mut r, style, false, ex);
        }
        let size = (r.right - r.left, r.bottom - r.top);
        if size != self.size {
            self.size = size;
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    size.0,
                    size.1,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }

    pub fn close(self, reg: &mut Reg) {
        reg.destroy(self.hwnd);
    }
}

pub unsafe extern "system" fn panel_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = container_msg(h, msg, w, l) {
        return r;
    }
    match msg {
        WM_ERASEBKGND => erase(h, HDC(w.0 as *mut _)),
        WM_PAINT => paint_ground(h),
        // Escape: `IsDialogMessage` sends `IDCANCEL`, and Escape closes
        // the window as it does a dialog.
        WM_CLOSE | WM_COMMAND if msg == WM_CLOSE || w.0 == IDCANCEL.0 as usize => {
            // The view takes the panel away; the window goes when it does.
            let handler = with_state(|s| {
                let k = s.panels.iter().find(|p| p.hwnd == h)?.key.clone();
                Some(s.view.panel(&k)?.on_close.clone())
            })
            .flatten();
            if let Some(handler) = handler {
                handler.call(());
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            look::forget_paint(key(h));
            unsafe { DefWindowProcW(h, msg, w, l) }
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}
