//! The view's dialogs as modal windows laid out like a task dialog: a
//! headline, a message, the body's controls, and a footer of buttons.
//!
//! A real `TaskDialog` cannot hold arbitrary controls (a text field, a
//! combo box) and its buttons cannot be enabled as a form fills in, so the
//! window is built by hand, with the same proportions.

use std::path::PathBuf;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::{DialogStyle, Role};

use crate::app::{main_window, with_state, Bind, Reg};
use crate::controls::*;
use crate::look::{self, Font, Paint};
use crate::pane::Pane;
use crate::util::*;

pub const DIALOG_CLASS: windows::core::PCWSTR = w!("WtmDialog");

// In DIPs.
const PAD: f64 = 22.0;
const MIN_WIDTH: f64 = 360.0;
const MAX_WIDTH: f64 = 660.0;
const ICON: f64 = 32.0;
const FOOTER: f64 = 48.0;

pub struct Dialog {
    pub id: u64,
    pub hwnd: HWND,
    tip: HWND,
    icon: HWND,
    headline: HWND,
    message: HWND,
    body: Option<Pane>,
    buttons: Vec<HWND>,
    roles: Vec<Role>,
    placed: std::collections::HashMap<isize, (RECT, bool)>,
}

impl Dialog {
    pub fn open(owner: HWND, d: &wtm_toolkit::Dialog, reg: &mut Reg) -> Dialog {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
                DIALOG_CLASS,
                &HSTRING::from(text_of(owner)),
                WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
                0,
                0,
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
        let icon = match d.style {
            DialogStyle::Info => HWND::default(),
            style => {
                let h = create(w!("STATIC"), "", WS_CHILD_ | 3, WINDOW_EX_STYLE(0), hwnd);
                let id = if style == DialogStyle::Critical {
                    IDI_ERROR
                } else {
                    IDI_WARNING
                };
                if let Ok(i) = unsafe { LoadIconW(None, id) } {
                    send(h, STM_SETICON, i.0 as usize, 0);
                }
                look::set_paint(
                    key(h),
                    Paint::Plain {
                        ground: pal.window,
                        ink: pal.text,
                    },
                );
                h
            }
        };
        // The headline in the task dialog's main-instruction style.
        let headline = label(
            hwnd,
            "",
            Font::Instruction,
            SS_LEFT_ | SS_EDITCONTROL_,
            pal.window,
            blend(pal.accent, pal.text, 0.35),
        );
        let message = label(
            hwnd,
            "",
            Font::Body,
            SS_LEFT_ | SS_EDITCONTROL_,
            pal.window,
            pal.text,
        );
        let mut dialog = Dialog {
            id: d.id,
            hwnd,
            tip,
            icon,
            headline,
            message,
            body: None,
            buttons: Vec::new(),
            roles: Vec::new(),
            placed: Default::default(),
        };
        dialog.patch(d, reg);
        // Centred over the window it belongs to.
        let mut o = RECT::default();
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(owner, &mut o);
            let _ = GetWindowRect(hwnd, &mut r);
        }
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let work = crate::app::work_area(owner);
        let x =
            (o.left + (o.right - o.left - w) / 2).clamp(work.left, (work.right - w).max(work.left));
        let y =
            (o.top + (o.bottom - o.top - h) / 3).clamp(work.top, (work.bottom - h).max(work.top));
        unsafe {
            let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            // Shown and active before the window under it is disabled:
            // disabling the active window first leaves no window active,
            // and the dialog may then not be allowed to take the
            // foreground.
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            // Modal: the window under it takes no input until it closes.
            let _ = EnableWindow(owner, false);
        }
        dialog.focus_first(d);
        dialog
    }

    fn focus_first(&self, d: &wtm_toolkit::Dialog) {
        let target = d
            .focus
            .and_then(|id| self.body.as_ref()?.control(id))
            .or_else(|| self.buttons.first().copied());
        if let Some(h) = target {
            unsafe {
                let _ = SetFocus(Some(h));
            }
            send(h, EM_SETSEL, 0, -1);
        }
    }

    pub fn patch(&mut self, d: &wtm_toolkit::Dialog, reg: &mut Reg) {
        set_text_if(self.headline, &d.title);
        set_text_if(self.message, &d.message);
        match (&d.body, &mut self.body) {
            (Some(e), Some(p)) => p.render(e, reg),
            (Some(e), None) => {
                let mut p = Pane::new(self.hwnd, self.tip, look::palette().window);
                p.render(e, reg);
                self.body = Some(p);
            }
            (None, Some(_)) => {
                if let Some(p) = self.body.take() {
                    p.destroy(reg);
                }
            }
            (None, None) => {}
        }
        let roles: Vec<Role> = d.buttons.iter().map(|b| b.role).collect();
        if roles != self.roles {
            for h in self.buttons.drain(..) {
                reg.destroy(h);
            }
            let face = look::palette().face;
            for _ in &d.buttons {
                self.buttons
                    .push(push_button(self.hwnd, "", Font::Body, face));
            }
            self.roles = roles;
        }
        for (i, (h, b)) in self.buttons.iter().zip(&d.buttons).enumerate() {
            set_text_if(*h, &b.label);
            enable(*h, b.enabled);
            let style = if i == 0 {
                BS_DEFPUSHBUTTON_
            } else {
                BS_PUSHBUTTON_
            };
            send(*h, BM_SETSTYLE, style as usize, 1);
            reg.bind(*h, Bind::DialogButton(i));
        }
        self.layout();
    }

    fn layout(&mut self) {
        let p = look::px;
        let pad = p(PAD);
        // As wide as the body, or the message on one line, within limits.
        let message_w = look::text_size(Font::Body, &text_of(self.message), None, false).0;
        let natural = self
            .body
            .as_ref()
            .map(|b| b.measure(f64::INFINITY).width.ceil() as i32)
            .unwrap_or(0)
            .max(message_w);
        let width = natural.clamp(p(MIN_WIDTH), p(MAX_WIDTH));
        let indent = if self.icon.is_invalid() {
            0
        } else {
            p(ICON) + p(12.0)
        };
        let text_w = width - indent;
        let mut placed = std::mem::take(&mut self.placed);
        let mut batch = Batch::new(&mut placed);
        let mut y = pad;
        if !self.icon.is_invalid() {
            batch.place(self.icon, rect(pad, y, p(ICON), p(ICON)));
        }
        let headline = text_of(self.headline);
        let (_, hh) = look::text_size(Font::Instruction, &headline, Some(text_w), true);
        batch.place(self.headline, rect(pad + indent, y, text_w, hh));
        y += hh;
        let message = text_of(self.message);
        if message.is_empty() {
            batch.hide(self.message);
        } else {
            y += p(8.0);
            let (_, mh) = look::text_size(Font::Body, &message, Some(text_w), true);
            batch.place(self.message, rect(pad + indent, y, text_w, mh));
            y += mh;
        }
        if !self.icon.is_invalid() {
            y = y.max(pad + p(ICON));
        }
        batch.apply();
        if let Some(b) = &mut self.body {
            y += p(16.0);
            let h = b.measure(width as f64).height.ceil() as i32;
            b.place(rect(pad, y, width, h));
            y += h;
        }
        y += pad;
        let footer_top = y;
        let footer = p(FOOTER);
        let total_h = footer_top + footer;
        let total_w = width + 2 * pad;

        // Buttons: in the view's order from the right edge's group; a
        // `Normal` one (Remove Repo…) stands apart at the far left.
        let mut batch = Batch::new(&mut placed);
        let bh = p(23.0);
        let by = footer_top + (footer - bh) / 2;
        let widths: Vec<i32> = self
            .buttons
            .iter()
            .map(|h| {
                (look::text_size(Font::Body, &text_of(*h), None, false).0 + p(24.0)).max(p(80.0))
            })
            .collect();
        let main: Vec<usize> = (0..self.buttons.len())
            .filter(|i| self.roles[*i] != Role::Normal)
            .collect();
        let main_w: i32 =
            main.iter().map(|i| widths[*i]).sum::<i32>() + p(8.0) * (main.len() as i32 - 1).max(0);
        let mut x = total_w - pad - main_w;
        for i in &main {
            batch.place(self.buttons[*i], rect(x, by, widths[*i], bh));
            x += widths[*i] + p(8.0);
        }
        let mut lx = pad;
        for i in (0..self.buttons.len()).filter(|i| self.roles[*i] == Role::Normal) {
            batch.place(self.buttons[i], rect(lx, by, widths[i], bh));
            lx += widths[i] + p(8.0);
        }
        batch.apply();
        self.placed = placed;
        // Where the footer starts, for the background painting.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, footer_top as isize);
        }
        let mut r = rect(0, 0, total_w, total_h);
        unsafe {
            let style = WINDOW_STYLE(GetWindowLongW(self.hwnd, GWL_STYLE) as u32);
            let ex = WINDOW_EX_STYLE(GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32);
            let _ = AdjustWindowRectEx(&mut r, style, false, ex);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        invalidate(self.hwnd);
    }

    /// Close without running anything: the view dropped it, or a button's
    /// handler is about to run.
    pub fn close(self, owner: HWND, reg: &mut Reg) {
        unsafe {
            // The owner is enabled first, so activation goes back to it
            // rather than to whatever window is behind.
            let _ = EnableWindow(owner, true);
        }
        reg.destroy(self.hwnd);
    }

    fn index_of(&self, h: HWND) -> Option<usize> {
        self.buttons.iter().position(|b| *b == h)
    }

    /// The button Escape and the close box press: the `Cancel` one, or the
    /// only one there is.
    fn cancel_index(&self) -> Option<usize> {
        self.roles
            .iter()
            .position(|r| *r == Role::Cancel)
            .or_else(|| (self.roles.len() == 1).then_some(0))
    }
}

/// The open dialog's button `i` was pressed: it closes, then its handler
/// runs.
pub fn press(i: usize) {
    let handler = with_state(|s| {
        let d = s.dialog.as_ref()?;
        let id = d.id;
        let button = s
            .view
            .dialogs
            .iter()
            .find(|x| x.id == id)?
            .buttons
            .get(i)?
            .clone();
        if !button.enabled {
            return None;
        }
        let d = s.dialog.take()?;
        s.closed_dialogs.insert(id);
        d.close(s.main, &mut s.reg);
        Some(button.on_press)
    })
    .flatten();
    if let Some(h) = handler {
        h.call(());
    }
}

fn cancel() {
    if let Some(Some(i)) = with_state(|s| s.dialog.as_ref().and_then(|d| d.cancel_index())) {
        press(i);
    }
}

pub fn is_dialog(h: HWND) -> bool {
    with_state(|s| s.dialog.as_ref().is_some_and(|d| d.hwnd == h)).unwrap_or(false)
}

fn dropped_combo(focus: HWND) -> bool {
    // The edit of an editable combo box has the keyboard, not the box.
    let combo = unsafe { GetParent(focus).unwrap_or_default() };
    [focus, combo].iter().any(|h| {
        !h.is_invalid() && send(*h, CB_GETDROPPEDSTATE, 0, 0) == 1 && class_is(*h, "ComboBox")
    })
}

fn class_is(h: HWND, name: &str) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize]).eq_ignore_ascii_case(name)
}

/// Return, Escape and Tab for the dialog `root`.
pub fn pre_translate(root: HWND, m: &MSG) -> bool {
    if m.message == WM_KEYDOWN {
        let vk = VIRTUAL_KEY(m.wParam.0 as u16);
        let focus = unsafe { GetFocus() };
        if (vk == VK_ESCAPE || vk == VK_RETURN) && dropped_combo(focus) {
            // Closing the combo box's list comes first.
            return unsafe { IsDialogMessageW(root, m) }.as_bool();
        }
        if vk == VK_ESCAPE {
            cancel();
            return true;
        }
        if vk == VK_RETURN {
            let i = with_state(|s| {
                let d = s.dialog.as_ref()?;
                Some(d.index_of(focus).unwrap_or(0))
            })
            .flatten();
            if let Some(i) = i {
                press(i);
            }
            return true;
        }
    }
    unsafe { IsDialogMessageW(root, m) }.as_bool()
}

/// The window colour over the footer's face colour, a hairline between.
fn paint_back(h: HWND, dc: HDC) {
    let c = client(h);
    let pal = look::palette();
    // Where the footer starts is kept in the window's user data.
    let top = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as i32;
    let top = if top > 0 { top } else { c.bottom };
    look::fill(dc, &RECT { bottom: top, ..c }, pal.window);
    look::fill(dc, &RECT { top, ..c }, pal.face);
    look::fill(
        dc,
        &RECT {
            top,
            bottom: top + 1,
            ..c
        },
        pal.card_border,
    );
}

pub unsafe extern "system" fn dialog_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = container_msg(h, msg, w, l) {
        return r;
    }
    match msg {
        WM_ERASEBKGND => {
            paint_back(h, HDC(w.0 as *mut _));
            LRESULT(1)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                let dc = BeginPaint(h, &mut ps);
                paint_back(h, dc);
                let _ = EndPaint(h, &ps);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            cancel();
            LRESULT(0)
        }
        WM_DESTROY => {
            look::forget_paint(key(h));
            unsafe { DefWindowProcW(h, msg, w, l) }
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}

// MARK: The folder picker

/// Ask for folders; empty when cancelled. Runs the picker's own modal
/// loop, so it is only ever called from a posted callback.
pub fn pick_folders(owner: HWND, title: &str, multiple: bool) -> Vec<PathBuf> {
    let owner = if owner.is_invalid() {
        main_window()
    } else {
        owner
    };
    unsafe {
        let Ok(dlg) =
            CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
        else {
            log::error!("the folder picker is not available");
            return Vec::new();
        };
        let mut opts = dlg.GetOptions().unwrap_or_default();
        opts |= FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        if multiple {
            opts |= FOS_ALLOWMULTISELECT;
        }
        let _ = dlg.SetOptions(opts);
        let _ = dlg.SetTitle(&HSTRING::from(title));
        // Cancelling comes back as an error.
        if dlg.Show(Some(owner)).is_err() {
            return Vec::new();
        }
        let Ok(items) = dlg.GetResults() else {
            return Vec::new();
        };
        let n = items.GetCount().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..n {
            let Ok(item) = items.GetItemAt(i) else {
                continue;
            };
            if let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                if let Ok(s) = name.to_string() {
                    out.push(PathBuf::from(s));
                }
                CoTaskMemFree(Some(name.0 as *const _));
            }
        }
        out
    }
}
