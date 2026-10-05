//! How things look: the DPI scale, fonts, the palette, brushes, and the
//! drawing every owner-drawn control and the card list share (agent marks,
//! icon glyphs, rich text, anti-aliased shapes through GDI+).
//!
//! All of it lives in [`LOOK`], apart from the backend's state: painting and
//! `WM_CTLCOLOR*` arrive while a render is changing controls (a control
//! repaints itself from inside `SetWindowText`), and must still find their
//! colours. `LOOK` is only ever borrowed for the length of a lookup, never
//! across a call into Windows.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};
use wtm_toolkit::marks::{self, Paint as MarkPaint, Shape};
use wtm_toolkit::{Hue, Icon, Mark, Rich, Span, TextAlign, TextStyle, Tint};

use crate::util::Color;
use crate::util::*;

// MARK: State

pub struct Look {
    /// Device pixels per DIP (96 dpi = 1).
    pub scale: f64,
    pub fonts: HashMap<Font, HFONT>,
    /// The icon font's face, when Windows has one (Segoe Fluent Icons on
    /// Windows 11, Segoe MDL2 Assets on 10). `None`: icons are text labels.
    pub icon_face: Option<&'static str>,
    pub palette: Palette,
    brushes: HashMap<Color, HBRUSH>,
    /// How each owner-drawn or coloured control is drawn, by handle.
    pub paint: HashMap<isize, Paint>,
    /// The control under the pointer, for hover feedback.
    pub hover: isize,
    pub list: ListLook,
    /// The main window's own strips (toolbar, notice, status), painted
    /// behind its controls.
    pub bands: Vec<(RECT, Color)>,
}

/// What the card list's painter needs, in content coordinates.
#[derive(Default)]
pub struct ListLook {
    pub cards: Vec<Card>,
    pub selected: Option<String>,
    pub focused: bool,
    /// Where a dragged card would land.
    pub drop_line: Option<i32>,
    pub dragging: Option<String>,
}

pub struct Card {
    pub key: String,
    /// The card's outline.
    pub rect: RECT,
    /// The bottom of the header band.
    pub band_bottom: i32,
    pub plates: Vec<(String, RECT)>,
}

thread_local! {
    pub static LOOK: RefCell<Look> = RefCell::new(Look::new(1.0));
}

pub fn with<R>(f: impl FnOnce(&mut Look) -> R) -> R {
    LOOK.with(|l| f(&mut l.borrow_mut()))
}

pub fn scale() -> f64 {
    with(|l| l.scale)
}

/// `v` DIPs in device pixels.
pub fn px(v: f64) -> i32 {
    (v * scale()).round() as i32
}

pub fn palette() -> Palette {
    with(|l| l.palette)
}

pub fn font(f: Font) -> HFONT {
    with(|l| l.fonts.get(&f).copied().unwrap_or_default())
}

pub fn brush(c: Color) -> HBRUSH {
    with(|l| {
        *l.brushes
            .entry(c)
            .or_insert_with(|| unsafe { CreateSolidBrush(colorref(c)) })
    })
}

pub fn set_paint(h: isize, p: Paint) {
    with(|l| {
        l.paint.insert(h, p);
    })
}

pub fn paint_of(h: isize) -> Option<Paint> {
    with(|l| l.paint.get(&h).cloned())
}

pub fn forget_paint(h: isize) {
    with(|l| {
        l.paint.remove(&h);
        if l.hover == h {
            l.hover = 0;
        }
    })
}

pub fn icons_available() -> bool {
    with(|l| l.icon_face.is_some())
}

impl Look {
    fn new(scale: f64) -> Self {
        Look {
            scale,
            fonts: HashMap::new(),
            icon_face: None,
            palette: Palette::system(),
            brushes: HashMap::new(),
            paint: HashMap::new(),
            hover: 0,
            list: ListLook::default(),
            bands: Vec::new(),
        }
    }
}

/// Build fonts, palette and brushes for `dpi`, replacing any from before
/// (a DPI or system colour change).
pub fn reset(dpi: u32) {
    let scale = dpi as f64 / 96.0;
    let icon_face = find_icon_face();
    let fonts = make_fonts(scale, icon_face);
    let (old_fonts, old_brushes) = with(|l| {
        l.scale = scale;
        l.icon_face = icon_face;
        l.palette = Palette::system();
        (
            std::mem::replace(&mut l.fonts, fonts),
            std::mem::take(&mut l.brushes),
        )
    });
    unsafe {
        for f in old_fonts.into_values() {
            let _ = DeleteObject(f.into());
        }
        for b in old_brushes.into_values() {
            let _ = DeleteObject(b.into());
        }
    }
}

// MARK: Palette

/// System colours and blends of them. Badge hues are the one exception:
/// Windows has no system green or orange, so they are Fluent's, laid on a
/// pale fill of themselves.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub window: Color,
    pub text: Color,
    pub face: Color,
    pub secondary: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub red: Color,
    /// Behind the cards.
    pub list: Color,
    pub card_border: Color,
    /// A repo card's header band: the window colour washed with a greyed
    /// accent, the Windows cousin of the macOS card's Aqua tint.
    pub band: Color,
    /// The recessed ground under a card's worktree plates.
    pub well: Color,
    pub plate: Color,
    pub plate_border: Color,
    pub notice: Color,
    pub chrome: Color,
}

impl Palette {
    pub fn system() -> Self {
        let window = sys(COLOR_WINDOW);
        let text = sys(COLOR_WINDOWTEXT);
        let face = sys(COLOR_BTNFACE);
        let accent = sys(COLOR_HIGHLIGHT);
        let gray = sys(COLOR_GRAYTEXT);
        let slate = blend(accent, gray, 0.45);
        Palette {
            window,
            text,
            face,
            secondary: blend(text, window, 0.42),
            accent,
            accent_text: sys(COLOR_HIGHLIGHTTEXT),
            red: rgb(0xc4, 0x2b, 0x1c),
            list: blend(face, window, 0.25),
            card_border: blend(face, text, 0.2),
            band: blend(blend(window, slate, 0.12), text, 0.02),
            well: blend(face, text, 0.03),
            plate: window,
            plate_border: blend(window, text, 0.16),
            notice: blend(window, accent, 0.08),
            chrome: face,
        }
    }

    pub fn ink(&self, ink: wtm_toolkit::Ink) -> Color {
        match ink {
            wtm_toolkit::Ink::Primary => self.text,
            wtm_toolkit::Ink::Secondary => self.secondary,
            wtm_toolkit::Ink::Error => self.red,
        }
    }

    pub fn hue(&self, hue: Hue) -> Color {
        match hue {
            Hue::Green => rgb(0x10, 0x7c, 0x10),
            Hue::Blue => rgb(0x00, 0x63, 0xb1),
            Hue::Orange => rgb(0xca, 0x50, 0x10),
            Hue::Purple => rgb(0x87, 0x64, 0xb8),
            Hue::Teal => rgb(0x03, 0x83, 0x87),
            Hue::Red => self.red,
            Hue::Yellow => rgb(0x9d, 0x5d, 0x00),
            Hue::Gray => blend(self.text, self.window, 0.45),
            Hue::Accent => self.accent,
        }
    }
}

// MARK: Fonts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    Body,
    BodyBold,
    Small,
    SmallBold,
    Caption,
    CaptionBold,
    Title,
    Heading,
    /// A dialog's headline, as Task Dialogs set theirs.
    Instruction,
    Path,
    Branch,
    BranchBold,
    Badge,
    Icon,
    IconSmall,
}

/// The font for a text style; `strong` runs are its bold.
pub fn style_font(style: TextStyle, strong: bool) -> Font {
    match (style, strong) {
        (TextStyle::Body, false) => Font::Body,
        (TextStyle::Body, true) => Font::BodyBold,
        (TextStyle::Heading, _) => Font::Heading,
        (TextStyle::Title, _) => Font::Title,
        (TextStyle::Small, false) => Font::Small,
        (TextStyle::Small, true) => Font::SmallBold,
        (TextStyle::Caption, false) => Font::Caption,
        (TextStyle::Caption, true) => Font::CaptionBold,
        (TextStyle::Path, _) => Font::Path,
        (TextStyle::Branch, false) => Font::Branch,
        (TextStyle::Branch, true) | (TextStyle::BranchStrong, _) => Font::BranchBold,
    }
}

/// The message font: what Windows itself uses in dialogs and controls.
fn message_font() -> LOGFONTW {
    let mut m = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            m.cbSize,
            Some(&mut m as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    let mut lf = m.lfMessageFont;
    if ok.is_err() || lf.lfFaceName[0] == 0 {
        lf = LOGFONTW::default();
        set_face(&mut lf, "Segoe UI");
    }
    lf
}

fn set_face(lf: &mut LOGFONTW, face: &str) {
    lf.lfFaceName = [0; 32];
    for (i, c) in face.encode_utf16().take(31).enumerate() {
        lf.lfFaceName[i] = c;
    }
}

fn make_fonts(scale: f64, icon_face: Option<&'static str>) -> HashMap<Font, HFONT> {
    let base = message_font();
    // Sizes in pixels at 96 dpi. The message font is 12 (Segoe UI 9 pt);
    // the rest keep the macOS styles' proportions to it, nudged up where
    // Windows' small text would be smaller than its own controls'.
    let make = |px: f64, weight: i32, face: Option<&str>, mono: bool| {
        let mut lf = base;
        lf.lfHeight = -((px * scale).round() as i32);
        lf.lfWidth = 0;
        lf.lfWeight = weight;
        lf.lfItalic = 0;
        lf.lfUnderline = 0;
        lf.lfQuality = CLEARTYPE_QUALITY;
        if let Some(face) = face {
            set_face(&mut lf, face);
        }
        if mono {
            // Asked by family too, so a system without Consolas falls back
            // to another fixed-pitch face rather than the UI font.
            lf.lfPitchAndFamily = FIXED_PITCH.0 | FF_MODERN.0;
        }
        unsafe { CreateFontIndirectW(&lf) }
    };
    let regular = 400;
    let semibold = 600;
    let bold = 700;
    let mut f = HashMap::new();
    f.insert(Font::Body, make(12.0, regular, None, false));
    f.insert(Font::BodyBold, make(12.0, bold, None, false));
    f.insert(Font::Small, make(12.0, regular, None, false));
    f.insert(Font::SmallBold, make(12.0, bold, None, false));
    f.insert(Font::Caption, make(11.0, regular, None, false));
    f.insert(Font::CaptionBold, make(11.0, bold, None, false));
    f.insert(Font::Title, make(15.0, semibold, None, false));
    f.insert(Font::Heading, make(18.0, semibold, None, false));
    f.insert(Font::Instruction, make(16.0, regular, None, false));
    f.insert(Font::Path, make(11.5, regular, Some("Consolas"), true));
    f.insert(Font::Branch, make(13.0, regular, Some("Consolas"), true));
    f.insert(Font::BranchBold, make(13.0, bold, Some("Consolas"), true));
    f.insert(Font::Badge, make(11.0, semibold, None, false));
    if let Some(face) = icon_face {
        f.insert(Font::Icon, make(14.0, regular, Some(face), false));
        f.insert(Font::IconSmall, make(12.0, regular, Some(face), false));
    }
    f
}

/// The first icon font installed, by trying each face with the font
/// enumerator: GDI never fails to create a font, it substitutes, so asking
/// for one is no test.
fn find_icon_face() -> Option<&'static str> {
    unsafe extern "system" fn found(
        _: *const LOGFONTW,
        _: *const TEXTMETRICW,
        _: u32,
        l: LPARAM,
    ) -> i32 {
        unsafe { *(l.0 as *mut bool) = true };
        0
    }
    let dc = unsafe { GetDC(None) };
    let mut out = None;
    for face in ["Segoe Fluent Icons", "Segoe MDL2 Assets"] {
        let mut lf = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        set_face(&mut lf, face);
        let mut hit = false;
        unsafe {
            EnumFontFamiliesExW(dc, &lf, Some(found), LPARAM(&mut hit as *mut _ as isize), 0);
        }
        if hit {
            out = Some(face);
            break;
        }
    }
    unsafe { ReleaseDC(None, dc) };
    out
}

// MARK: Measuring text

/// A DC for measuring, released on drop.
pub struct ScreenDc(pub HDC);

impl ScreenDc {
    pub fn new() -> Self {
        ScreenDc(unsafe { GetDC(None) })
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe { ReleaseDC(None, self.0) };
    }
}

/// The size of `text` in `f`: on one line, or wrapped to `max_width`.
pub fn text_size(f: Font, text: &str, max_width: Option<i32>, wrap: bool) -> (i32, i32) {
    let dc = ScreenDc::new();
    let h = font(f);
    unsafe {
        let old = SelectObject(dc.0, h.into());
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        if buf.is_empty() {
            buf.push(' ' as u16);
        }
        let mut r = RECT {
            left: 0,
            top: 0,
            right: max_width.unwrap_or(100_000),
            bottom: 0,
        };
        let flags = DT_CALCRECT
            | DT_NOPREFIX
            | if wrap && max_width.is_some() {
                // As `SS_EDITCONTROL` wraps: a word longer than the line (a
                // path) is broken rather than run past the edge.
                DT_WORDBREAK | DT_EDITCONTROL
            } else {
                DT_SINGLELINE
            };
        DrawTextW(dc.0, &mut buf, &mut r, flags);
        SelectObject(dc.0, old);
        let w = if text.is_empty() { 0 } else { r.right - r.left };
        (w, r.bottom - r.top)
    }
}

/// The height of one line of `f`.
pub fn line_height(f: Font) -> i32 {
    text_size(f, "Ag", None, false).1
}

pub fn mark_size() -> i32 {
    px(marks::MARK_SIZE)
}

pub fn mark_gap() -> i32 {
    px(marks::MARK_GAP)
}

/// The width and height of `rich` set in `style`, on one line.
pub fn rich_size(rich: &Rich, style: TextStyle) -> (i32, i32) {
    let mut w = 0;
    let mut h = line_height(style_font(style, false));
    for s in &rich.spans {
        match s {
            Span::Text { text, strong } => {
                let (tw, th) = text_size(style_font(style, *strong), text, None, false);
                w += tw;
                h = h.max(th);
            }
            Span::Mark(_) => {
                w += mark_size() + mark_gap();
                h = h.max(mark_size());
            }
        }
    }
    (w, h)
}

pub fn has_marks_or_emphasis(rich: &Rich) -> bool {
    rich.spans.iter().any(|s| match s {
        Span::Mark(_) => true,
        Span::Text { strong, .. } => *strong,
    })
}

// MARK: Drawing

pub fn fill(dc: HDC, r: &RECT, c: Color) {
    unsafe { FillRect(dc, r, brush(c)) };
}

/// Draw `text` in `f` within `r`.
pub fn draw_text(dc: HDC, f: Font, text: &str, r: RECT, ink: Color, flags: DRAW_TEXT_FORMAT) {
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    if buf.is_empty() {
        return;
    }
    let mut r = r;
    unsafe {
        let old = SelectObject(dc, font(f).into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, colorref(ink));
        DrawTextW(dc, &mut buf, &mut r, flags | DT_NOPREFIX);
        SelectObject(dc, old);
    }
}

/// Draw `rich` on one line in `r`, vertically centred, marks drawn in
/// place of the agent prefixes. Text past the right edge ends in an
/// ellipsis.
pub fn draw_rich(
    dc: HDC,
    rich: &Rich,
    style: TextStyle,
    r: RECT,
    ink: Color,
    ground: Color,
    align: TextAlign,
) {
    let (w, _) = rich_size(rich, style);
    let mut x = match align {
        TextAlign::Leading => r.left,
        TextAlign::Trailing => (r.right - w).max(r.left),
        TextAlign::Center => r.left + ((r.right - r.left - w) / 2).max(0),
    };
    for s in &rich.spans {
        if x >= r.right {
            break;
        }
        match s {
            Span::Text { text, strong } => {
                let f = style_font(style, *strong);
                let (tw, _) = text_size(f, text, None, false);
                let cell = RECT {
                    left: x,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                };
                draw_text(
                    dc,
                    f,
                    text,
                    cell,
                    ink,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_LEFT,
                );
                x += tw;
            }
            Span::Mark(m) => {
                let size = mark_size();
                let y = r.top + (r.bottom - r.top - size) / 2;
                draw_mark(dc, *m, x, y, size, ink, ground);
                x += size + mark_gap();
            }
        }
    }
}

/// GDI+ on a DC, anti-aliased, for shapes GDI draws jagged.
/// A line from one point to another.
pub type Segment = ((f32, f32), (f32, f32));

pub struct Gfx(*mut GpGraphics);

impl Gfx {
    pub fn new(dc: HDC) -> Option<Self> {
        let mut g = std::ptr::null_mut();
        unsafe {
            if GdipCreateFromHDC(dc, &mut g) != Ok || g.is_null() {
                return None;
            }
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
        }
        Some(Gfx(g))
    }

    fn with_path(&self, build: impl FnOnce(*mut GpPath), use_path: impl FnOnce(*mut GpPath)) {
        let mut path = std::ptr::null_mut();
        unsafe {
            if GdipCreatePath(FillModeAlternate, &mut path) != Ok {
                return;
            }
            build(path);
            use_path(path);
            GdipDeletePath(path);
        }
    }

    fn round_rect(path: *mut GpPath, x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) {
        // Corners clockwise from the top left; GDI+ joins each to the next.
        // A zero radius is a square corner, added as a point.
        let corners = [
            (x, y, 180.0, (x, y)),
            (x + w - 2.0 * r[1], y, 270.0, (x + w, y)),
            (x + w - 2.0 * r[2], y + h - 2.0 * r[2], 0.0, (x + w, y + h)),
            (x, y + h - 2.0 * r[3], 90.0, (x, y + h)),
        ];
        unsafe {
            for (i, (cx, cy, start, (px, py))) in corners.into_iter().enumerate() {
                if r[i] > 0.0 {
                    GdipAddPathArc(path, cx, cy, r[i] * 2.0, r[i] * 2.0, start, 90.0);
                } else {
                    GdipAddPathLine(path, px, py, px, py);
                }
            }
            GdipClosePathFigure(path);
        }
    }

    /// Fill a rectangle with corners of radii `r` (top-left, top-right,
    /// bottom-right, bottom-left).
    pub fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, r: [f32; 4], argb: u32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let g = self.0;
        self.with_path(
            |p| Self::round_rect(p, x, y, w, h, r),
            |p| unsafe {
                let mut b = std::ptr::null_mut();
                if GdipCreateSolidFill(argb, &mut b) == Ok {
                    GdipFillPath(g, b as *mut GpBrush, p);
                    GdipDeleteBrush(b as *mut GpBrush);
                }
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_round(&self, x: f32, y: f32, w: f32, h: f32, r: [f32; 4], width: f32, argb: u32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let g = self.0;
        self.with_path(
            |p| Self::round_rect(p, x, y, w, h, r),
            |p| unsafe {
                let mut pen = std::ptr::null_mut();
                if GdipCreatePen1(argb, width, UnitPixel, &mut pen) == Ok {
                    GdipDrawPath(g, pen, p);
                    GdipDeletePen(pen);
                }
            },
        );
    }

    pub fn fill_polygon(&self, points: &[(f32, f32)], argb: u32) {
        let pts: Vec<PointF> = points.iter().map(|&(x, y)| PointF { X: x, Y: y }).collect();
        unsafe {
            let mut b = std::ptr::null_mut();
            if GdipCreateSolidFill(argb, &mut b) == Ok {
                GdipFillPolygon(
                    self.0,
                    b as *mut GpBrush,
                    pts.as_ptr(),
                    pts.len() as i32,
                    FillModeAlternate,
                );
                GdipDeleteBrush(b as *mut GpBrush);
            }
        }
    }

    pub fn lines(&self, segments: &[Segment], width: f32, argb: u32) {
        unsafe {
            let mut pen = std::ptr::null_mut();
            if GdipCreatePen1(argb, width, UnitPixel, &mut pen) == Ok {
                for ((x1, y1), (x2, y2)) in segments {
                    GdipDrawLine(self.0, pen, *x1, *y1, *x2, *y2);
                }
                GdipDeletePen(pen);
            }
        }
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.0);
        }
    }
}

#[allow(non_upper_case_globals)]
const Ok: Status = Status(0);

/// Start GDI+ for the life of the process.
pub fn start_gdiplus() {
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        ..Default::default()
    };
    let mut token = 0usize;
    unsafe {
        let _ = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
    }
}

/// An agent mark, `size` pixels square at (`x`, `y`), from the shared
/// shapes; `Ink` paints take the surrounding text's colour over `ground`.
pub fn draw_mark(dc: HDC, mark: Mark, x: i32, y: i32, size: i32, ink: Color, ground: Color) {
    let Some(g) = Gfx::new(dc) else { return };
    let k = size as f32 / 16.0;
    let (ox, oy) = (x as f32, y as f32);
    let color = |p: MarkPaint| match p {
        MarkPaint::Fixed(c) => argb(
            rgb(
                (c.0 * 255.0).round() as u8,
                (c.1 * 255.0).round() as u8,
                (c.2 * 255.0).round() as u8,
            ),
            1.0,
        ),
        MarkPaint::Ink(alpha) => argb(blend(ground, ink, alpha), 1.0),
    };
    for shape in marks::shapes(mark) {
        match shape {
            Shape::RoundedRect {
                x,
                y,
                w,
                h,
                r,
                paint,
            } => {
                let r = (r as f32 * k).min(w as f32 * k / 2.0);
                g.fill_round(
                    ox + x as f32 * k,
                    oy + y as f32 * k,
                    w as f32 * k,
                    h as f32 * k,
                    [r; 4],
                    color(paint),
                );
            }
            Shape::Polygon { points, paint } => {
                let pts: Vec<(f32, f32)> = points
                    .iter()
                    .map(|(px, py)| (ox + *px as f32 * k, oy + *py as f32 * k))
                    .collect();
                g.fill_polygon(&pts, color(paint));
            }
            Shape::Lines {
                segments,
                width,
                paint,
            } => {
                let segs: Vec<_> = segments
                    .iter()
                    .map(|((x1, y1), (x2, y2))| {
                        (
                            (ox + *x1 as f32 * k, oy + *y1 as f32 * k),
                            (ox + *x2 as f32 * k, oy + *y2 as f32 * k),
                        )
                    })
                    .collect();
                g.lines(&segs, (width as f32 * k).max(1.0), color(paint));
            }
        }
    }
}

// MARK: Icons

/// The Segoe icon font's glyph for each icon. Both fonts share the code
/// points used here.
pub fn glyph(icon: Icon) -> char {
    match icon {
        Icon::Add => '\u{E710}',
        Icon::Refresh => '\u{E72C}',
        Icon::Settings => '\u{E713}',
        Icon::Copy => '\u{E8C8}',
        Icon::Push => '\u{E898}',
        Icon::Pull => '\u{E896}',
        Icon::Merge => '\u{E8B5}',
        Icon::Editor => '\u{E943}',
        Icon::Terminal => '\u{E756}',
        Icon::Folder => '\u{E838}',
        Icon::Delete => '\u{E74D}',
        Icon::Close => '\u{E711}',
        Icon::Chevrons => '\u{E70D}',
    }
}

/// What a button says in place of its glyph when there is no icon font.
pub fn icon_label(icon: Icon) -> &'static str {
    match icon {
        Icon::Add => "+",
        Icon::Refresh => "Refresh",
        Icon::Settings => "Settings",
        Icon::Copy => "Copy",
        Icon::Push => "Push",
        Icon::Pull => "Pull",
        Icon::Merge => "Merge",
        Icon::Editor => "Editor",
        Icon::Terminal => "Terminal",
        Icon::Folder => "Reveal",
        Icon::Delete => "Delete",
        Icon::Close => "\u{00D7}",
        Icon::Chevrons => "\u{25BE}",
    }
}

/// The size of an icon button's content: the glyph, or its label.
pub fn icon_size(icon: Icon, small: bool) -> (i32, i32) {
    if icons_available() {
        let s = px(if small { 20.0 } else { 24.0 });
        (s, s)
    } else {
        let (w, h) = text_size(Font::Caption, icon_label(icon), None, false);
        (w + px(10.0), h.max(px(20.0)))
    }
}

pub fn draw_icon(dc: HDC, icon: Icon, r: RECT, ink: Color, small: bool) {
    let flags = DT_SINGLELINE | DT_CENTER | DT_VCENTER;
    if icons_available() {
        let f = if small { Font::IconSmall } else { Font::Icon };
        draw_text(dc, f, &glyph(icon).to_string(), r, ink, flags);
    } else {
        draw_text(dc, Font::Caption, icon_label(icon), r, ink, flags);
    }
}

/// A chevron drawn as two strokes: pointing down (`open`) or right.
pub fn draw_chevron(dc: HDC, r: RECT, ink: Color, open: bool, half: f32) {
    let Some(g) = Gfx::new(dc) else { return };
    let cx = (r.left + r.right) as f32 / 2.0;
    let cy = (r.top + r.bottom) as f32 / 2.0;
    let h = half;
    let segs = if open {
        [
            ((cx - h, cy - h / 2.0), (cx, cy + h / 2.0)),
            ((cx, cy + h / 2.0), (cx + h, cy - h / 2.0)),
        ]
    } else {
        [
            ((cx - h / 2.0, cy - h), (cx + h / 2.0, cy)),
            ((cx + h / 2.0, cy), (cx - h / 2.0, cy + h)),
        ]
    };
    g.lines(&segs, (1.6 * scale() as f32).max(1.0), argb(ink, 1.0));
}

// MARK: Owner-drawn controls

/// How a control is coloured or drawn. Every control in a coloured area
/// has one, if only for its ground.
#[derive(Debug, Clone)]
pub enum Paint {
    /// A label, edit or standard button: its ground and text colour, for
    /// `WM_CTLCOLOR*`.
    Plain { ground: Color, ink: Color },
    /// A borderless glyph button.
    Icon {
        icon: Icon,
        ground: Color,
        tint: Tint,
        small: bool,
    },
    /// A toolbar button: glyph and label, flat until hovered.
    Tool {
        icon: Icon,
        label: String,
        ground: Color,
    },
    /// The branch switcher: a bezel holding the branch (marks drawn) and a
    /// chevron.
    Pill { rich: Rich, ground: Color },
    /// A card's disclosure chevron.
    Disclosure { open: bool, ground: Color },
    Badge {
        text: String,
        hue: Hue,
        ground: Color,
    },
    /// A label whose text carries marks or emphasis.
    Rich {
        rich: Rich,
        style: TextStyle,
        ink: Color,
        ground: Color,
        align: TextAlign,
    },
    /// The branch picker's rows.
    List {
        items: Vec<wtm_toolkit::ListItem>,
        ground: Color,
    },
}

impl Paint {
    pub fn ground(&self) -> Color {
        match self {
            Paint::Plain { ground, .. }
            | Paint::Icon { ground, .. }
            | Paint::Tool { ground, .. }
            | Paint::Pill { ground, .. }
            | Paint::Disclosure { ground, .. }
            | Paint::Badge { ground, .. }
            | Paint::Rich { ground, .. }
            | Paint::List { ground, .. } => *ground,
        }
    }
}

/// The pill's inner padding and the room its chevron takes.
pub fn pill_extra() -> i32 {
    px(8.0) * 2 + px(12.0)
}

pub fn draw_owner(
    dc: HDC,
    r: RECT,
    paint: &Paint,
    hovered: bool,
    pressed: bool,
    enabled: bool,
    focused: bool,
) {
    let pal = palette();
    let ground = paint.ground();
    fill(dc, &r, ground);
    let (x, y, w, h) = (
        r.left as f32,
        r.top as f32,
        (r.right - r.left) as f32,
        (r.bottom - r.top) as f32,
    );
    let corner = (4.0 * scale()) as f32;
    let hover_fill = |g: &Gfx| {
        if pressed {
            g.fill_round(
                x,
                y,
                w,
                h,
                [corner; 4],
                argb(blend(ground, pal.text, 0.14), 1.0),
            );
        } else if hovered && enabled {
            g.fill_round(
                x,
                y,
                w,
                h,
                [corner; 4],
                argb(blend(ground, pal.text, 0.07), 1.0),
            );
        }
    };
    match paint {
        Paint::Plain { .. } => {}
        Paint::Icon {
            icon, tint, small, ..
        } => {
            if let Some(g) = Gfx::new(dc) {
                hover_fill(&g);
            }
            let ink = if !enabled {
                blend(ground, pal.text, 0.3)
            } else if *tint == Tint::Danger {
                pal.red
            } else {
                blend(pal.text, ground, 0.3)
            };
            draw_icon(dc, *icon, r, ink, *small);
        }
        Paint::Tool { icon, label, .. } => {
            if let Some(g) = Gfx::new(dc) {
                hover_fill(&g);
            }
            let ink = if enabled {
                pal.text
            } else {
                blend(ground, pal.text, 0.4)
            };
            let pad = px(8.0);
            let mut text = r;
            if icons_available() {
                let icon_r = RECT {
                    left: r.left + pad,
                    right: r.left + pad + px(18.0),
                    ..r
                };
                draw_icon(dc, *icon, icon_r, ink, true);
                text.left = icon_r.right + px(6.0);
            } else {
                text.left += pad;
            }
            draw_text(dc, Font::Body, label, text, ink, DT_SINGLELINE | DT_VCENTER);
        }
        Paint::Pill { rich, .. } => {
            if let Some(g) = Gfx::new(dc) {
                let fill_c = if pressed {
                    blend(ground, pal.text, 0.12)
                } else if hovered && enabled {
                    blend(ground, pal.text, 0.07)
                } else {
                    blend(ground, pal.text, 0.035)
                };
                g.fill_round(x, y, w, h, [corner * 1.5; 4], argb(fill_c, 1.0));
                g.stroke_round(
                    x + 0.5,
                    y + 0.5,
                    w - 1.0,
                    h - 1.0,
                    [corner * 1.5; 4],
                    1.0,
                    argb(blend(ground, pal.text, 0.16), 1.0),
                );
            }
            let ink = if enabled {
                pal.text
            } else {
                blend(ground, pal.text, 0.4)
            };
            let pad = px(8.0);
            let chevron_w = px(12.0);
            let text_r = RECT {
                left: r.left + pad,
                right: r.right - pad - chevron_w,
                ..r
            };
            draw_rich(
                dc,
                rich,
                TextStyle::Branch,
                text_r,
                ink,
                ground,
                TextAlign::Leading,
            );
            let chev = RECT {
                left: r.right - pad - chevron_w + px(2.0),
                right: r.right - pad,
                ..r
            };
            draw_chevron(
                dc,
                chev,
                blend(ink, ground, 0.25),
                true,
                3.0 * scale() as f32,
            );
        }
        Paint::Disclosure { open, .. } => {
            if let Some(g) = Gfx::new(dc) {
                hover_fill(&g);
            }
            draw_chevron(
                dc,
                r,
                blend(pal.text, ground, 0.35),
                *open,
                4.0 * scale() as f32,
            );
        }
        Paint::Badge { text, hue, .. } => {
            let c = pal.hue(*hue);
            if let Some(g) = Gfx::new(dc) {
                g.fill_round(x, y, w, h, [h / 2.0; 4], argb(blend(ground, c, 0.16), 1.0));
            }
            draw_text(
                dc,
                Font::Badge,
                text,
                r,
                blend(c, pal.text, 0.15),
                DT_SINGLELINE | DT_CENTER | DT_VCENTER,
            );
        }
        Paint::Rich {
            rich,
            style,
            ink,
            align,
            ..
        } => draw_rich(dc, rich, *style, r, *ink, ground, *align),
        Paint::List { .. } => {}
    }
    if focused {
        let mut f = r;
        let inset = px(1.0);
        f.left += inset;
        f.top += inset;
        f.right -= inset;
        f.bottom -= inset;
        unsafe {
            SetTextColor(dc, colorref(pal.text));
            SetBkColor(dc, colorref(ground));
            let _ = DrawFocusRect(dc, &f);
        }
    }
}
