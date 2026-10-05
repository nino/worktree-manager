//! Creating and patching standard controls, tooltips, hover tracking, the
//! spinner, and what every container window does with its controls'
//! notifications, owner drawing and colours.

use std::cell::Cell;
use std::collections::HashMap;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::look::{self, Font, Paint};
use crate::util::*;

// Styles as plain bits: the crate types them per control family.
pub const WS_CHILD_: u32 = WS_CHILD.0;
pub const WS_VISIBLE_: u32 = WS_VISIBLE.0;
pub const WS_TABSTOP_: u32 = WS_TABSTOP.0;
pub const BS_PUSHBUTTON_: u32 = 0;
pub const BS_DEFPUSHBUTTON_: u32 = 1;
pub const BS_OWNERDRAW_: u32 = 11;
pub const BS_RADIOBUTTON_: u32 = 4;
pub const BS_PUSHLIKE_: u32 = 0x1000;
// A static's type (left, right, centred, no-wrap, owner-drawn, ...) is one
// value in the low five bits, not a set of flags: `SS_RIGHT | SS_LEFTNOWORDWRAP`
// is `SS_BITMAP`. Only the ellipsis and notify styles combine.
pub const SS_LEFT_: u32 = 0;
pub const SS_CENTER_: u32 = 1;
pub const SS_RIGHT_: u32 = 2;
pub const SS_OWNERDRAW_: u32 = 13;
pub const SS_LEFTNOWORDWRAP_: u32 = 12;
pub const SS_NOPREFIX_: u32 = 0x80;
/// Wraps as a multi-line edit does, breaking a word too long for a line.
pub const SS_EDITCONTROL_: u32 = 0x2000;
pub const SS_NOTIFY_: u32 = 0x100;
pub const SS_ENDELLIPSIS_: u32 = 0x4000;
pub const SS_PATHELLIPSIS_: u32 = 0x8000;
pub const ES_MULTILINE_: u32 = 4;
pub const ES_AUTOHSCROLL_: u32 = 0x80;
pub const ES_AUTOVSCROLL_: u32 = 0x40;
pub const ES_READONLY_: u32 = 0x800;
pub const ES_NOHIDESEL_: u32 = 0x100;
pub const CBS_DROPDOWN_: u32 = 2;
pub const CBS_DROPDOWNLIST_: u32 = 3;
pub const CBS_AUTOHSCROLL_: u32 = 0x40;
pub const PBS_MARQUEE_: u32 = 8;

/// A private message: run the closures queued for the UI thread.
pub const WM_APP_RUN: u32 = WM_APP + 1;
/// A private message: run the UI-thread-only work queued by `later`.
pub const WM_APP_LATER: u32 = WM_APP + 2;
/// A private message: the menu closed; rebuild it if a render had to wait.
pub const WM_APP_MENU: u32 = WM_APP + 3;
/// A private message: hand the keyboard to the main window's controls if
/// activation came back to it while the state was borrowed.
pub const WM_APP_FOCUS: u32 = WM_APP + 4;
/// A private message: the list scrolled; the picker follows its anchor.
pub const WM_APP_FOLLOW: u32 = WM_APP + 5;
/// A private message: compare the main window's DPI with the one the
/// fonts were made for, after a DPI change arrived while the state was
/// borrowed.
pub const WM_APP_DPI: u32 = WM_APP + 6;

pub fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).map(Into::into).unwrap_or_default() }
}

thread_local! {
    static NEXT_ID: Cell<usize> = const { Cell::new(1000) };
}

pub fn create(class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, parent: HWND) -> HWND {
    let text = wide(text);
    // Child ids are unique so `WM_COMMAND` never confuses two controls; the
    // handle is what notifications are routed by.
    let id = NEXT_ID.with(|n| {
        let v = n.get();
        n.set(if v >= 0xEFFF { 1000 } else { v + 1 });
        v
    });
    let child = style & WS_CHILD_ != 0;
    unsafe {
        CreateWindowExW(
            ex,
            class,
            pcwstr(&text),
            WINDOW_STYLE(style),
            0,
            0,
            0,
            0,
            Some(parent),
            if child {
                Some(HMENU(id as *mut _))
            } else {
                None
            },
            Some(instance()),
            None,
        )
        .unwrap_or_default()
    }
}

pub fn set_font(h: HWND, f: Font) {
    send(h, WM_SETFONT, look::font(f).0 as usize, 1);
}

pub fn show(h: HWND, visible: bool) {
    unsafe {
        let _ = ShowWindow(h, if visible { SW_SHOWNA } else { SW_HIDE });
    }
}

pub fn visible(h: HWND) -> bool {
    unsafe { IsWindowVisible(h).as_bool() }
}

pub fn enable(h: HWND, on: bool) {
    unsafe {
        if IsWindowEnabled(h).as_bool() != on {
            let _ = EnableWindow(h, on);
        }
    }
}

/// Whether `h`, or a control inside it (a combo box's edit), has the
/// keyboard.
pub fn has_focus(h: HWND) -> bool {
    let f = unsafe { GetFocus() };
    !f.is_invalid() && (f == h || unsafe { IsChild(h, f).as_bool() })
}

/// Enable a field unless it would take the keyboard away from someone
/// typing in it.
pub fn enable_field(h: HWND, on: bool) {
    if on || !has_focus(h) {
        enable(h, on);
    }
}

pub fn set_text_if(h: HWND, s: &str) {
    if text_of(h) != s {
        set_text(h, s);
    }
}

/// A field's text from the view, unless the user is typing in it: the view
/// can lag a keystroke behind, and rewriting the text would move the caret
/// and drop what was just typed.
pub fn set_field_text(h: HWND, s: &str) {
    if !has_focus(h) {
        set_text_if(h, s);
    }
}

pub fn invalidate(h: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(h), None, false);
    }
}

pub fn client(h: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(h, &mut r);
    }
    r
}

/// Multi-line edits want `\r\n`.
pub fn crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

// MARK: Making controls

/// A standard push button (the system draws it, so it is themed).
pub fn push_button(parent: HWND, label: &str, f: Font, ground: Color) -> HWND {
    let h = create(
        w!("BUTTON"),
        label,
        WS_CHILD_ | WS_TABSTOP_ | BS_PUSHBUTTON_,
        WINDOW_EX_STYLE(0),
        parent,
    );
    set_font(h, f);
    look::set_paint(
        key(h),
        Paint::Plain {
            ground,
            ink: look::palette().text,
        },
    );
    h
}

/// An owner-drawn button: icon buttons, the pill, toolbar buttons, the
/// disclosure chevron. Its window text is what assistive technology reads.
pub fn owner_button(parent: HWND, name: &str, paint: Paint) -> HWND {
    let h = create(
        w!("BUTTON"),
        name,
        WS_CHILD_ | WS_TABSTOP_ | BS_OWNERDRAW_,
        WINDOW_EX_STYLE(0),
        parent,
    );
    look::set_paint(key(h), paint);
    track_hover(h);
    h
}

/// A plain label.
pub fn label(parent: HWND, text: &str, f: Font, style: u32, ground: Color, ink: Color) -> HWND {
    let h = create(
        w!("STATIC"),
        text,
        WS_CHILD_ | SS_NOPREFIX_ | SS_NOTIFY_ | style,
        WINDOW_EX_STYLE(0),
        parent,
    );
    set_font(h, f);
    look::set_paint(key(h), Paint::Plain { ground, ink });
    h
}

/// An owner-drawn label (badges, text with marks).
pub fn owner_label(parent: HWND, text: &str, paint: Paint) -> HWND {
    let h = create(
        w!("STATIC"),
        text,
        WS_CHILD_ | SS_OWNERDRAW_ | SS_NOTIFY_,
        WINDOW_EX_STYLE(0),
        parent,
    );
    look::set_paint(key(h), paint);
    h
}

pub fn set_paint_ink(h: HWND, ink: Color) {
    look::with(|l| {
        if let Some(Paint::Plain { ink: i, .. }) = l.paint.get_mut(&key(h)) {
            *i = ink;
        }
    });
}

/// Replace an owner-drawn control's look, repainting only when it changed.
pub fn repaint_with(h: HWND, paint: Paint) {
    let changed = look::with(|l| {
        let k = key(h);
        let same = l
            .paint
            .get(&k)
            .is_some_and(|p| format!("{p:?}") == format!("{paint:?}"));
        l.paint.insert(k, paint);
        !same
    });
    if changed {
        invalidate(h);
    }
}

// MARK: Tooltips

/// A tooltip window for the controls of `owner`'s window.
pub fn tooltip(owner: HWND) -> HWND {
    let h = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            PCWSTR::null(),
            WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            Some(owner),
            None,
            Some(instance()),
            None,
        )
        .unwrap_or_default()
    };
    // Long hints (a notice, a full path) wrap rather than run off screen.
    send(h, TTM_SETMAXTIPWIDTH, 0, look::px(420.0) as isize);
    h
}

fn tool_info(ctl: HWND, text: &mut Vec<u16>) -> TTTOOLINFOW {
    TTTOOLINFOW {
        cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd: unsafe { GetParent(ctl).unwrap_or_default() },
        uId: ctl.0 as usize,
        lpszText: windows::core::PWSTR(text.as_mut_ptr()),
        ..Default::default()
    }
}

/// What each control's tooltip says, and which tooltip window holds it.
pub type Tips = HashMap<isize, (isize, String)>;

/// Give `ctl` the tooltip `text` (`None` or empty: none). `known` holds
/// what each control was last given, so an unchanged hint costs nothing.
pub fn set_tip(tip: HWND, known: &mut Tips, ctl: HWND, text: Option<&str>) {
    let text = text.unwrap_or("");
    let k = key(ctl);
    let before = known.get(&k).map(|(_, t)| t.as_str());
    if before == Some(text) || (before.is_none() && text.is_empty()) {
        return;
    }
    let mut buf = wide(text);
    let info = tool_info(ctl, &mut buf);
    let p = &info as *const _ as isize;
    match (before, text.is_empty()) {
        (_, true) => {
            send(tip, TTM_DELTOOLW, 0, p);
            known.remove(&k);
        }
        (None, false) => {
            send(tip, TTM_ADDTOOLW, 0, p);
            known.insert(k, (key(tip), text.to_string()));
        }
        (Some(_), false) => {
            send(tip, TTM_UPDATETIPTEXTW, 0, p);
            known.insert(k, (key(tip), text.to_string()));
        }
    }
}

/// Take `ctl`'s tool out of its tooltip, before the control goes.
pub fn drop_tip(known: &mut Tips, ctl: HWND) {
    if let Some((tip, _)) = known.remove(&key(ctl)) {
        let mut buf = wide("");
        let info = tool_info(ctl, &mut buf);
        send(hwnd(tip), TTM_DELTOOLW, 0, &info as *const _ as isize);
    }
}

/// Pass a label's clicks and drags on to the list under it: a label that
/// shows a tooltip has to take the mouse (`SS_NOTIFY`), and the list must
/// still see a press on a card's text as a press on the card.
pub fn forward_mouse(h: HWND) {
    unsafe {
        let _ = SetWindowSubclass(h, Some(forward_proc), 2, 0);
    }
}

unsafe extern "system" fn forward_proc(
    h: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_MOUSEMOVE => {
            let (x, y) = point_of(l);
            let mut pt = [windows::Win32::Foundation::POINT { x, y }];
            unsafe {
                let parent = GetParent(h).unwrap_or_default();
                MapWindowPoints(Some(h), Some(parent), &mut pt);
                let packed = ((pt[0].y as u16 as u32) << 16 | pt[0].x as u16 as u32) as isize;
                SendMessageW(parent, msg, Some(w), Some(LPARAM(packed)));
                if msg == WM_MOUSEMOVE {
                    return DefSubclassProc(h, msg, w, l);
                }
            }
            LRESULT(0)
        }
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(h, Some(forward_proc), id);
            DefSubclassProc(h, msg, w, l)
        },
        _ => unsafe { DefSubclassProc(h, msg, w, l) },
    }
}

// MARK: Hover

/// Track the pointer over an owner-drawn button, for its hover look.
pub fn track_hover(h: HWND) {
    unsafe {
        let _ = SetWindowSubclass(h, Some(hover_proc), 1, 0);
    }
}

unsafe extern "system" fn hover_proc(
    h: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let k = key(h);
            if look::with(|lk| std::mem::replace(&mut lk.hover, k)) != k {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut t);
                }
                invalidate(h);
            }
        }
        WM_MOUSELEAVE => {
            look::with(|lk| {
                if lk.hover == key(h) {
                    lk.hover = 0;
                }
            });
            invalidate(h);
        }
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(h, Some(hover_proc), id);
            look::forget_paint(key(h));
        },
        _ => {}
    }
    unsafe { DefSubclassProc(h, msg, w, l) }
}

// MARK: Spinner

pub const SPINNER_CLASS: PCWSTR = w!("WtmSpinner");
const SPIN_TIMER: usize = 1;
const SPOKES: isize = 8;

/// A small round spinner that keeps its place when stopped.
pub fn spinner(parent: HWND, ground: Color) -> HWND {
    let h = create(SPINNER_CLASS, "", WS_CHILD_, WINDOW_EX_STYLE(0), parent);
    look::set_paint(
        key(h),
        Paint::Plain {
            ground,
            ink: look::palette().text,
        },
    );
    h
}

pub fn set_spinning(h: HWND, on: bool) {
    let was = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } & 0x100 != 0;
    if was == on {
        return;
    }
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, if on { 0x100 } else { 0 });
        if on {
            SetTimer(Some(h), SPIN_TIMER, 90, None);
        } else {
            let _ = KillTimer(Some(h), SPIN_TIMER);
        }
    }
    invalidate(h);
}

pub unsafe extern "system" fn spinner_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            let v = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) };
            unsafe { SetWindowLongPtrW(h, GWLP_USERDATA, 0x100 | (((v & 0xff) + 1) % SPOKES)) };
            invalidate(h);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = client(h);
            let paint = look::paint_of(key(h));
            let pal = look::palette();
            let ground = paint.map(|p| p.ground()).unwrap_or(pal.window);
            look::fill(dc, &r, ground);
            let v = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) };
            if v & 0x100 != 0 {
                if let Some(g) = look::Gfx::new(dc) {
                    let cx = (r.left + r.right) as f32 / 2.0;
                    let cy = (r.top + r.bottom) as f32 / 2.0;
                    let outer = (r.right - r.left).min(r.bottom - r.top) as f32 / 2.0 - 0.5;
                    let inner = outer * 0.45;
                    let head = (v & 0xff) as f32;
                    for i in 0..SPOKES {
                        let a = std::f32::consts::TAU * i as f32 / SPOKES as f32;
                        // The spoke at the head is darkest; the rest fade
                        // behind it.
                        let age = (head - i as f32).rem_euclid(SPOKES as f32) / SPOKES as f32;
                        let ink = blend(pal.text, ground, 0.25 + 0.6 * age as f64);
                        g.lines(
                            &[(
                                (cx + inner * a.sin(), cy - inner * a.cos()),
                                (cx + outer * a.sin(), cy - outer * a.cos()),
                            )],
                            (outer * 0.28).max(1.2),
                            argb(ink, 1.0),
                        );
                    }
                }
            }
            unsafe {
                let _ = EndPaint(h, &ps);
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

// MARK: Containers

/// What every window holding controls does with them: route their
/// notifications, draw the owner-drawn ones, colour the rest. `None` for a
/// message that is none of these.
pub fn container_msg(_h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_COMMAND if l.0 != 0 => {
            crate::app::command(hwnd(l.0), hiword(w.0) as u32);
            Some(LRESULT(0))
        }
        WM_DRAWITEM => {
            let d = unsafe { &*(l.0 as *const DRAWITEMSTRUCT) };
            draw_item(d);
            Some(LRESULT(1))
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = HDC(w.0 as *mut _);
            let paint = look::paint_of(l.0)?;
            let (ground, ink) = match paint {
                Paint::Plain { ground, ink } => (ground, ink),
                other => (other.ground(), look::palette().text),
            };
            unsafe {
                SetTextColor(dc, colorref(ink));
                SetBkColor(dc, colorref(ground));
            }
            Some(LRESULT(look::brush(ground).0 as isize))
        }
        _ => None,
    }
}

fn draw_item(d: &DRAWITEMSTRUCT) {
    let k = key(d.hwndItem);
    let Some(paint) = look::paint_of(k) else {
        return;
    };
    if let Paint::List { items, ground } = &paint {
        crate::popover::draw_row(d, items, *ground);
        return;
    }
    let state = d.itemState;
    let hovered = look::with(|l| l.hover == k);
    let is_button = d.CtlType == ODT_BUTTON;
    look::draw_owner(
        d.hDC,
        d.rcItem,
        &paint,
        hovered,
        is_button && state.0 & ODS_SELECTED.0 != 0,
        state.0 & ODS_DISABLED.0 == 0 && unsafe { IsWindowEnabled(d.hwndItem).as_bool() },
        is_button && state.0 & ODS_FOCUS.0 != 0 && state.0 & ODS_NOFOCUSRECT.0 == 0,
    );
}

/// Fill a container's background with its ground colour.
pub fn erase(h: HWND, dc: HDC) -> LRESULT {
    let ground = look::paint_of(key(h))
        .map(|p| p.ground())
        .unwrap_or_else(|| look::palette().window);
    look::fill(dc, &client(h), ground);
    LRESULT(1)
}

pub const PANE_CLASS: PCWSTR = w!("WtmPane");

pub unsafe extern "system" fn pane_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = container_msg(h, msg, w, l) {
        return r;
    }
    match msg {
        WM_ERASEBKGND => erase(h, HDC(w.0 as *mut _)),
        WM_PAINT => paint_ground(h),
        WM_DESTROY => {
            look::forget_paint(key(h));
            unsafe { DefWindowProcW(h, msg, w, l) }
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}

/// Paint what is to be painted with the window's ground. A child that
/// shrank or moved leaves its old place invalid in the parent without
/// always asking for it to be erased (Wine never does), so the ground is
/// painted here as well as on `WM_ERASEBKGND`.
pub fn paint_ground(h: HWND) -> LRESULT {
    let mut ps = PAINTSTRUCT::default();
    unsafe {
        let dc = BeginPaint(h, &mut ps);
        let ground = look::paint_of(key(h))
            .map(|p| p.ground())
            .unwrap_or_else(|| look::palette().window);
        look::fill(dc, &ps.rcPaint, ground);
        let _ = EndPaint(h, &ps);
    }
    LRESULT(0)
}

// MARK: Placing many controls at once

/// Moves and shows or hides a set of sibling controls in one
/// `DeferWindowPos` pass, skipping any already where they should be.
pub struct Batch<'a> {
    known: &'a mut HashMap<isize, (RECT, bool)>,
    moves: Vec<(HWND, RECT, bool)>,
}

impl<'a> Batch<'a> {
    pub fn new(known: &'a mut HashMap<isize, (RECT, bool)>) -> Self {
        Batch {
            known,
            moves: Vec::new(),
        }
    }

    pub fn place(&mut self, h: HWND, r: RECT) {
        self.set(h, r, true);
    }

    pub fn hide(&mut self, h: HWND) {
        let r = self.known.get(&key(h)).map(|k| k.0).unwrap_or_default();
        self.set(h, r, false);
    }

    pub fn set(&mut self, h: HWND, r: RECT, visible: bool) {
        if h.is_invalid() {
            return;
        }
        if self.known.get(&key(h)) == Some(&(r, visible)) && self::visible(h) == visible {
            return;
        }
        self.known.insert(key(h), (r, visible));
        self.moves.push((h, r, visible));
    }

    pub fn apply(self) {
        if self.moves.is_empty() {
            return;
        }
        unsafe {
            let mut dwp = BeginDeferWindowPos(self.moves.len() as i32).ok();
            for (h, r, vis) in &self.moves {
                // `SWP_NOCOPYBITS`: a moved control repaints rather than
                // having its old pixels copied over. A card dragged above
                // another moves every control in both, and copied bits
                // left pieces of each behind (seen under Wine).
                let flags = SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | SWP_NOCOPYBITS
                    | if *vis { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW };
                let next = dwp.and_then(|d| {
                    DeferWindowPos(
                        d,
                        *h,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        flags,
                    )
                    .ok()
                });
                if next.is_none() {
                    let _ = SetWindowPos(
                        *h,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        flags,
                    );
                }
                dwp = next;
            }
            if let Some(d) = dwp {
                let _ = EndDeferWindowPos(d);
            }
        }
    }
}
