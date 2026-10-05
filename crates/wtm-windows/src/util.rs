//! Small Win32 helpers: wide strings, window text, rectangles, colours.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetSysColor, SYS_COLOR_INDEX};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, SendMessageW, SetWindowTextW,
};

/// A NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Borrow a `wide` buffer as a `PCWSTR`. The buffer must outlive the call.
pub fn pcwstr(w: &[u16]) -> PCWSTR {
    PCWSTR(w.as_ptr())
}

/// A window handle as a map key: handles are not `Send`, `Hash` or stable
/// across a destroy, so maps key on the number.
pub fn key(h: HWND) -> isize {
    h.0 as isize
}

pub fn hwnd(k: isize) -> HWND {
    HWND(k as *mut core::ffi::c_void)
}

pub fn text_of(h: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(h).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetWindowTextW(h, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

/// Set a window's text. The control sends its change notification from
/// inside this call; containers ignore notifications while the backend is
/// rendering, which is what keeps a value the view set from being reported
/// back as the user's.
pub fn set_text(h: HWND, s: &str) {
    let w = wide(s);
    unsafe {
        let _ = SetWindowTextW(h, pcwstr(&w));
    }
}

pub fn send(h: HWND, msg: u32, w: usize, l: isize) -> isize {
    unsafe { SendMessageW(h, msg, Some(WPARAM(w)), Some(LPARAM(l))).0 }
}

pub fn loword(v: usize) -> u16 {
    (v & 0xffff) as u16
}

pub fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xffff) as u16
}

/// Signed coordinates packed in an `LPARAM` (mouse messages).
pub fn point_of(l: LPARAM) -> (i32, i32) {
    let v = l.0 as u32;
    (
        (v & 0xffff) as i16 as i32,
        ((v >> 16) & 0xffff) as i16 as i32,
    )
}

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

// MARK: Colours

/// A colour as `0x00BBGGRR`, the way GDI takes it.
pub type Color = u32;

pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

pub fn sys(index: SYS_COLOR_INDEX) -> Color {
    unsafe { GetSysColor(index) }
}

pub fn colorref(c: Color) -> COLORREF {
    COLORREF(c)
}

fn parts(c: Color) -> (f64, f64, f64) {
    (
        (c & 0xff) as f64,
        ((c >> 8) & 0xff) as f64,
        ((c >> 16) & 0xff) as f64,
    )
}

/// `a` moved `t` of the way towards `b`.
pub fn blend(a: Color, b: Color, t: f64) -> Color {
    let (ar, ag, ab) = parts(a);
    let (br, bg, bb) = parts(b);
    let mix = |x: f64, y: f64| (x + (y - x) * t).round().clamp(0.0, 255.0) as u8;
    rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// For GDI+, which takes `0xAARRGGBB`.
pub fn argb(c: Color, alpha: f64) -> u32 {
    let (r, g, b) = parts(c);
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    a << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}
