//! The branch picker: a borderless popup under a row's branch button, a
//! filter field over an owner-drawn list.
//!
//! The popup takes activation so its field reads the keyboard; losing it
//! (a click anywhere else, another app) is the user dismissing it. When the
//! view drops it, it is destroyed while the state is borrowed, and the
//! deactivation that follows is not reported.

use std::cell::Cell;
use std::time::Instant;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::{FilterList, ListItem, TextAlign, TextStyle};

use crate::app::{with_state, Bind, Reg};
use crate::controls::*;
use crate::look::{self, Font, Paint};
use crate::util::*;

pub const POPOVER_CLASS: windows::core::PCWSTR = w!("WtmPopover");

// In DIPs.
const WIDTH: f64 = 340.0;
const PAD: f64 = 8.0;
const FIELD: f64 = 26.0;
const ROW: f64 = 26.0;
const VISIBLE_ROWS: usize = 10;

pub struct Popover {
    pub id: u64,
    pub hwnd: HWND,
    edit: HWND,
    list: HWND,
    anchor: HWND,
    filter: FilterList,
    count: usize,
}

const LBS_NOTIFY_: u32 = 0x1;
const LBS_OWNERDRAWFIXED_: u32 = 0x10;
const LBS_NOINTEGRALHEIGHT_: u32 = 0x100;
const LBS_NODATA_: u32 = 0x2000;

impl Popover {
    pub fn open(owner: HWND, anchor: HWND, p: &wtm_toolkit::Popover, reg: &mut Reg) -> Popover {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                POPOVER_CLASS,
                w!("Branches"),
                WS_POPUP | WS_BORDER | WS_CLIPCHILDREN,
                0,
                0,
                10,
                10,
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
        let edit = create(
            w!("EDIT"),
            "",
            WS_CHILD_ | WS_VISIBLE_ | WS_TABSTOP_ | ES_AUTOHSCROLL_,
            WS_EX_CLIENTEDGE,
            hwnd,
        );
        set_font(edit, Font::Body);
        let cue = wide(&p.list.placeholder);
        send(edit, EM_SETCUEBANNER, 1, cue.as_ptr() as isize);
        unsafe {
            let _ = SetWindowSubclass(edit, Some(edit_proc), 3, 0);
        }
        let list = create(
            w!("LISTBOX"),
            "",
            WS_CHILD_
                | WS_VISIBLE_
                | WS_VSCROLL.0
                | LBS_NOTIFY_
                | LBS_OWNERDRAWFIXED_
                | LBS_NOINTEGRALHEIGHT_
                | LBS_NODATA_,
            WINDOW_EX_STYLE(0),
            hwnd,
        );
        send(list, LB_SETITEMHEIGHT, 0, look::px(ROW) as isize);
        let mut pop = Popover {
            id: p.id,
            hwnd,
            edit,
            list,
            anchor,
            filter: p.list.clone(),
            count: usize::MAX,
        };
        pop.patch(p, reg);
        pop.follow();
        OPEN.with(|o| o.set(true));
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetFocus(Some(edit));
        }
        pop
    }

    pub fn patch(&mut self, p: &wtm_toolkit::Popover, reg: &mut Reg) {
        let f = &p.list;
        set_text_if(self.edit, &f.query);
        repaint_with(
            self.list,
            Paint::List {
                items: f.items.clone(),
                ground: look::palette().window,
            },
        );
        if self.count != f.items.len() {
            self.count = f.items.len();
            send(self.list, LB_SETCOUNT, self.count, 0);
            self.follow();
        }
        let want = f.selected.map_or(-1, |i| i as isize);
        if send(self.list, LB_GETCURSEL, 0, 0) != want {
            send(self.list, LB_SETCURSEL, want as usize, 0);
        }
        invalidate(self.list);
        reg.bind(self.edit, Bind::Text(f.on_query.clone()));
        reg.bind(self.list, Bind::PickerList(f.on_choose.clone()));
        self.filter = f.clone();
    }

    /// Under the anchor, its left edges lined up, kept on its screen; the
    /// list as tall as its rows, up to a limit.
    pub fn follow(&mut self) {
        let p = look::px;
        let mut a = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.anchor, &mut a);
        }
        let rows = self.count.clamp(1, VISIBLE_ROWS) as i32;
        let width = p(WIDTH).max(a.right - a.left);
        let list_h = rows * p(ROW) + 2;
        let height = p(PAD) * 3 + p(FIELD) + list_h + 2;
        let work = crate::app::work_area(self.anchor);
        let x = a.left.clamp(work.left, (work.right - width).max(work.left));
        let below = a.bottom + p(2.0);
        let y = if below + height > work.bottom {
            // No room below: above the anchor instead.
            (a.top - p(2.0) - height).max(work.top)
        } else {
            below
        };
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let c = client(self.hwnd);
        let inner = c.right - 2 * p(PAD);
        unsafe {
            let _ = MoveWindow(self.edit, p(PAD), p(PAD), inner, p(FIELD), true);
            let _ = MoveWindow(
                self.list,
                p(PAD),
                p(PAD) * 2 + p(FIELD),
                inner,
                c.bottom - p(PAD) * 3 - p(FIELD),
                true,
            );
        }
    }

    /// Destroy it without reporting anything.
    pub fn close(self, reg: &mut Reg) {
        OPEN.with(|o| o.set(false));
        reg.destroy(self.hwnd);
    }
}

thread_local! {
    /// Shown now. Read while the state is borrowed (the main window's
    /// title bar asks during a render that opens the picker).
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

pub fn is_open() -> bool {
    OPEN.with(|o| o.get())
}

pub fn is_popover(h: HWND) -> bool {
    with_state(|s| s.popover.as_ref().is_some_and(|p| p.hwnd == h)).unwrap_or(false)
}

/// The user dismissed it: hidden at once (the view takes it away after
/// the message is handled) and reported.
fn dismiss(by_anchor_click: bool) {
    let handler = with_state(|s| {
        let p = s.popover.take()?;
        if by_anchor_click {
            s.swallow = Some((key(p.anchor), Instant::now()));
        }
        let h = p.filter.on_dismiss.clone();
        p.close(&mut s.reg);
        Some(h)
    })
    .flatten();
    if let Some(h) = handler {
        h.call(());
    }
}

/// The pointer is over `anchor`: a click there is what took activation.
fn pointer_on_anchor() -> bool {
    with_state(|s| {
        let p = s.popover.as_ref()?;
        let mut pt = Default::default();
        let mut r = RECT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
            let _ = GetWindowRect(p.anchor, &mut r);
        }
        Some(pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom)
    })
    .flatten()
    .unwrap_or(false)
}

fn key_action(vk: VIRTUAL_KEY) -> bool {
    let f = with_state(|s| s.popover.as_ref().map(|p| p.filter.clone())).flatten();
    let Some(f) = f else { return false };
    match vk {
        VK_UP => f.on_move.call(-1),
        VK_DOWN => f.on_move.call(1),
        VK_RETURN => {
            if let Some(i) = f.selected {
                f.on_choose.call(i);
            }
        }
        VK_ESCAPE => dismiss(false),
        _ => return false,
    }
    true
}

unsafe extern "system" fn edit_proc(
    h: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    match msg {
        WM_GETDLGCODE => LRESULT((DLGC_WANTALLKEYS | DLGC_HASSETSEL | DLGC_WANTCHARS) as isize),
        WM_KEYDOWN => {
            if key_action(VIRTUAL_KEY(w.0 as u16)) {
                LRESULT(0)
            } else {
                unsafe { DefSubclassProc(h, msg, w, l) }
            }
        }
        // The characters of Return and Escape would only beep.
        WM_CHAR if w.0 == 0x0D || w.0 == 0x1B => LRESULT(0),
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(h, Some(edit_proc), id);
            DefSubclassProc(h, msg, w, l)
        },
        _ => unsafe { DefSubclassProc(h, msg, w, l) },
    }
}

pub unsafe extern "system" fn popover_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = container_msg(h, msg, w, l) {
        return r;
    }
    match msg {
        WM_ACTIVATE => {
            if loword(w.0) as u32 == WA_INACTIVE {
                // A click on the anchor itself closes the picker, and that
                // click must not open it again.
                let on_anchor = pointer_on_anchor();
                dismiss(on_anchor);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => erase(h, HDC(w.0 as *mut _)),
        WM_PAINT => paint_ground(h),
        WM_MEASUREITEM => {
            let m = unsafe { &mut *(l.0 as *mut MEASUREITEMSTRUCT) };
            m.itemHeight = look::px(ROW) as u32;
            LRESULT(1)
        }
        WM_DESTROY => {
            look::forget_paint(key(h));
            unsafe { DefWindowProcW(h, msg, w, l) }
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}

/// One row of the list: a check for the current branch, then the name with
/// its agent mark.
pub fn draw_row(d: &DRAWITEMSTRUCT, items: &[ListItem], ground: Color) {
    let pal = look::palette();
    let r = d.rcItem;
    let Some(item) = items.get(d.itemID as usize) else {
        look::fill(d.hDC, &r, ground);
        return;
    };
    let selected = d.itemState.0 & ODS_SELECTED.0 != 0;
    let (bg, ink) = if selected {
        (pal.accent, pal.accent_text)
    } else {
        (ground, pal.text)
    };
    look::fill(d.hDC, &r, ground);
    if selected {
        if let Some(g) = look::Gfx::new(d.hDC) {
            let s = look::scale() as f32;
            g.fill_round(
                r.left as f32 + s,
                r.top as f32 + s,
                (r.right - r.left) as f32 - 2.0 * s,
                (r.bottom - r.top) as f32 - 2.0 * s,
                [4.0 * s; 4],
                argb(bg, 1.0),
            );
        }
    }
    let check = RECT {
        left: r.left + look::px(4.0),
        right: r.left + look::px(24.0),
        ..r
    };
    if item.checked {
        let mark = if look::icons_available() {
            "\u{E73E}"
        } else {
            "\u{2713}"
        };
        let f = if look::icons_available() {
            Font::IconSmall
        } else {
            Font::Body
        };
        look::draw_text(
            d.hDC,
            f,
            mark,
            check,
            ink,
            DT_SINGLELINE | DT_CENTER | DT_VCENTER,
        );
    }
    let text = RECT {
        left: check.right + look::px(4.0),
        right: r.right - look::px(6.0),
        ..r
    };
    look::draw_rich(
        d.hDC,
        &item.label,
        TextStyle::Branch,
        text,
        ink,
        if selected { bg } else { ground },
        TextAlign::Leading,
    );
}
