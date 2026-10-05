//! A generic [`Element`] tree as real controls in a container window: the
//! notice bar, the empty state, dialog bodies, the Settings window.
//!
//! One control per element, in [`Element::walk`] order, created once and
//! patched in place while the tree keeps its shape (so focus, a caret and a
//! selection survive every render); rebuilt only when the shape changes.
//! Placement is `wtm_toolkit::layout`'s.

use std::collections::HashMap;

use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::layout::{self, Measure, Metrics, Size};
use wtm_toolkit::{ButtonKind, Element, Id, TextAlign};

use crate::app::{Bind, Reg};
use crate::controls::*;
use crate::look::{self, Font, Paint};
use crate::util::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Label,
    Rich,
    Selectable,
    Button,
    IconButton,
    Badge,
    Spinner,
    Field,
    Combo,
    Choice,
    Block,
}

enum Node {
    None,
    One(HWND, Leaf),
    Segments(Vec<HWND>),
}

pub struct Pane {
    pub hwnd: HWND,
    tip: HWND,
    ground: Color,
    shape: Vec<String>,
    nodes: Vec<Node>,
    captions: Vec<Option<HWND>>,
    element: Option<Element>,
    placed: HashMap<isize, (RECT, bool)>,
    cues: HashMap<isize, String>,
    options: HashMap<isize, Vec<String>>,
}

/// What decides which control an element gets: an element whose key
/// changes needs a different control.
fn shape_of(e: &Element) -> Vec<String> {
    let mut out = Vec::new();
    e.walk(&mut |x| {
        out.push(match x {
            Element::Stack(s) => format!("stack {:?}", s.axis),
            Element::Text(t) => format!(
                "text {:?} {:?} {} {} {:?}",
                text_leaf(t),
                t.style,
                t.wrap,
                t.lines,
                t.align
            ),
            Element::Button(b) => format!("button {} {:?}", b.id, b.kind),
            Element::Badge(_) => "badge".into(),
            Element::Spinner(_) => "spinner".into(),
            Element::Field(f) => format!("field {}", f.id),
            Element::Combo(c) => format!("combo {}", c.id),
            Element::Segmented(s) => format!("segments {} {}", s.id, s.options.len()),
            Element::Choice(c) => format!("choice {}", c.id),
            Element::TextBlock(_) => "block".into(),
            Element::Form(f) => format!(
                "form {:?}",
                f.rows
                    .iter()
                    .map(|r| r.caption.is_empty())
                    .collect::<Vec<_>>()
            ),
            Element::Gap(_) => "gap".into(),
            Element::Flex => "flex".into(),
        })
    });
    out
}

fn text_leaf(t: &wtm_toolkit::Text) -> Leaf {
    if t.selectable {
        Leaf::Selectable
    } else if look::has_marks_or_emphasis(&t.content) {
        Leaf::Rich
    } else {
        Leaf::Label
    }
}

fn button_font(kind: ButtonKind) -> Font {
    match kind {
        ButtonKind::Push | ButtonKind::Pill => Font::Body,
        ButtonKind::Small | ButtonKind::Accessory | ButtonKind::Icon => Font::Small,
    }
}

impl Pane {
    pub fn new(parent: HWND, tip: HWND, ground: Color) -> Pane {
        let hwnd = create(
            PANE_CLASS,
            "",
            WS_CHILD_ | WS_VISIBLE_ | WS_CLIPCHILDREN.0,
            WS_EX_CONTROLPARENT,
            parent,
        );
        look::set_paint(
            key(hwnd),
            Paint::Plain {
                ground,
                ink: look::palette().text,
            },
        );
        Pane {
            hwnd,
            tip,
            ground,
            shape: Vec::new(),
            nodes: Vec::new(),
            captions: Vec::new(),
            element: None,
            placed: HashMap::new(),
            cues: HashMap::new(),
            options: HashMap::new(),
        }
    }

    pub fn destroy(self, reg: &mut Reg) {
        reg.destroy(self.hwnd);
    }

    /// The control showing the element with this id, if any.
    pub fn control(&self, id: Id) -> Option<HWND> {
        let e = self.element.as_ref()?;
        let mut i = 0;
        let mut found = None;
        e.walk(&mut |x| {
            let hit = match x {
                Element::Field(f) => f.id == id,
                Element::Combo(c) => c.id == id,
                Element::Choice(c) => c.id == id,
                Element::Segmented(s) => s.id == id,
                Element::Button(b) => b.id == id,
                Element::Text(t) => t.id == Some(id),
                _ => false,
            };
            if hit && found.is_none() {
                found = Some(i);
            }
            i += 1;
        });
        match self.nodes.get(found?)? {
            Node::One(h, _) => Some(*h),
            Node::Segments(v) => v.first().copied(),
            Node::None => None,
        }
    }

    /// Bring the controls in line with `e`.
    pub fn render(&mut self, e: &Element, reg: &mut Reg) {
        let shape = shape_of(e);
        if shape != self.shape {
            self.rebuild(e, reg);
            self.shape = shape;
        }
        let mut elements = Vec::new();
        e.walk(&mut |x| elements.push(x));
        for (i, x) in elements.into_iter().enumerate() {
            self.patch(i, x, reg);
        }
        let mut c = 0;
        e.walk(&mut |x| {
            if let Element::Form(f) = x {
                for r in &f.rows {
                    if let Some(Some(h)) = self.captions.get(c) {
                        set_text_if(*h, &r.caption);
                    }
                    c += 1;
                }
            }
        });
        self.element = Some(e.clone());
    }

    fn rebuild(&mut self, e: &Element, reg: &mut Reg) {
        for n in self.nodes.drain(..) {
            match n {
                Node::One(h, _) => reg.destroy(h),
                Node::Segments(v) => v.into_iter().for_each(|h| reg.destroy(h)),
                Node::None => {}
            }
        }
        for h in self.captions.drain(..).flatten() {
            reg.destroy(h);
        }
        self.placed.clear();
        self.cues.clear();
        self.options.clear();
        let parent = self.hwnd;
        let ground = self.ground;
        let pal = look::palette();
        let mut nodes = Vec::new();
        let mut captions = Vec::new();
        e.walk(&mut |x| {
            let node = match x {
                Element::Stack(_) | Element::Gap(_) | Element::Flex => Node::None,
                Element::Form(f) => {
                    for r in &f.rows {
                        captions.push((!r.caption.is_empty()).then(|| {
                            label(
                                parent,
                                &r.caption,
                                Font::Body,
                                SS_RIGHT_ | SS_ENDELLIPSIS_,
                                ground,
                                pal.text,
                            )
                        }));
                    }
                    Node::None
                }
                Element::Text(t) => {
                    let f = look::style_font(t.style, false);
                    let ink = pal.ink(t.ink);
                    match text_leaf(t) {
                        Leaf::Selectable => {
                            let multi = t.wrap || t.lines != 1;
                            let h = create(
                                w!("EDIT"),
                                "",
                                WS_CHILD_
                                    | ES_READONLY_
                                    | if multi {
                                        ES_MULTILINE_
                                    } else {
                                        ES_AUTOHSCROLL_
                                    },
                                WINDOW_EX_STYLE(0),
                                parent,
                            );
                            set_font(h, f);
                            look::set_paint(key(h), Paint::Plain { ground, ink });
                            Node::One(h, Leaf::Selectable)
                        }
                        Leaf::Rich => Node::One(
                            owner_label(
                                parent,
                                &t.content.to_plain(),
                                Paint::Rich {
                                    rich: t.content.clone(),
                                    style: t.style,
                                    ink,
                                    ground,
                                    align: t.align,
                                },
                            ),
                            Leaf::Rich,
                        ),
                        _ => {
                            let align = match t.align {
                                TextAlign::Leading => SS_LEFT_,
                                TextAlign::Trailing => SS_RIGHT_,
                                TextAlign::Center => SS_CENTER_,
                            };
                            let style = if t.wrap {
                                align | SS_EDITCONTROL_
                            } else if t.align == TextAlign::Leading {
                                SS_LEFTNOWORDWRAP_ | SS_ENDELLIPSIS_
                            } else {
                                align | SS_ENDELLIPSIS_
                            };
                            Node::One(label(parent, "", f, style, ground, ink), Leaf::Label)
                        }
                    }
                }
                Element::Button(b) => match b.kind {
                    ButtonKind::Icon => Node::One(
                        owner_button(
                            parent,
                            b.hint.as_deref().unwrap_or(""),
                            Paint::Icon {
                                icon: b.icon.unwrap_or(wtm_toolkit::Icon::Close),
                                ground,
                                tint: b.tint,
                                small: true,
                            },
                        ),
                        Leaf::IconButton,
                    ),
                    kind => Node::One(
                        push_button(parent, "", button_font(kind), ground),
                        Leaf::Button,
                    ),
                },
                Element::Badge(b) => Node::One(
                    owner_label(
                        parent,
                        &b.text,
                        Paint::Badge {
                            text: b.text.clone(),
                            hue: b.hue,
                            ground,
                        },
                    ),
                    Leaf::Badge,
                ),
                Element::Spinner(_) => Node::One(spinner(parent, ground), Leaf::Spinner),
                Element::Field(_) => {
                    let h = create(
                        w!("EDIT"),
                        "",
                        WS_CHILD_ | WS_TABSTOP_ | ES_AUTOHSCROLL_,
                        WS_EX_CLIENTEDGE,
                        parent,
                    );
                    set_font(h, Font::Body);
                    Node::One(h, Leaf::Field)
                }
                Element::Combo(_) => {
                    let h = create(
                        w!("COMBOBOX"),
                        "",
                        WS_CHILD_ | WS_TABSTOP_ | WS_VSCROLL.0 | CBS_DROPDOWN_ | CBS_AUTOHSCROLL_,
                        WINDOW_EX_STYLE(0),
                        parent,
                    );
                    set_font(h, Font::Body);
                    Node::One(h, Leaf::Combo)
                }
                Element::Choice(_) => {
                    let h = create(
                        w!("COMBOBOX"),
                        "",
                        WS_CHILD_ | WS_TABSTOP_ | WS_VSCROLL.0 | CBS_DROPDOWNLIST_,
                        WINDOW_EX_STYLE(0),
                        parent,
                    );
                    set_font(h, Font::Body);
                    look::set_paint(
                        key(h),
                        Paint::Plain {
                            ground,
                            ink: pal.text,
                        },
                    );
                    Node::One(h, Leaf::Choice)
                }
                Element::Segmented(s) => Node::Segments(
                    s.options
                        .iter()
                        .enumerate()
                        .map(|(i, o)| {
                            // A row of push-like radio buttons: one tab stop
                            // for the group, arrows within it.
                            let h = create(
                                w!("BUTTON"),
                                o,
                                WS_CHILD_
                                    | BS_RADIOBUTTON_
                                    | BS_PUSHLIKE_
                                    | if i == 0 { WS_TABSTOP_ | WS_GROUP.0 } else { 0 },
                                WINDOW_EX_STYLE(0),
                                parent,
                            );
                            set_font(h, Font::Body);
                            look::set_paint(
                                key(h),
                                Paint::Plain {
                                    ground,
                                    ink: pal.text,
                                },
                            );
                            h
                        })
                        .collect(),
                ),
                Element::TextBlock(_) => {
                    let h = create(
                        w!("EDIT"),
                        "",
                        WS_CHILD_
                            | WS_TABSTOP_
                            | WS_VSCROLL.0
                            | WS_HSCROLL.0
                            | ES_MULTILINE_
                            | ES_READONLY_
                            | ES_AUTOVSCROLL_
                            | ES_AUTOHSCROLL_
                            | ES_NOHIDESEL_,
                        WS_EX_CLIENTEDGE,
                        parent,
                    );
                    set_font(h, Font::Path);
                    look::set_paint(
                        key(h),
                        Paint::Plain {
                            ground: pal.window,
                            ink: pal.text,
                        },
                    );
                    Node::One(h, Leaf::Block)
                }
            };
            nodes.push(node);
        });
        self.nodes = nodes;
        self.captions = captions;
    }

    fn patch(&mut self, i: usize, e: &Element, reg: &mut Reg) {
        let pal = look::palette();
        let ground = self.ground;
        let tip = self.tip;
        match (&self.nodes[i], e) {
            (Node::One(h, leaf), Element::Text(t)) => {
                let h = *h;
                let ink = pal.ink(t.ink);
                match leaf {
                    Leaf::Rich => repaint_with(
                        h,
                        Paint::Rich {
                            rich: t.content.clone(),
                            style: t.style,
                            ink,
                            ground,
                            align: t.align,
                        },
                    ),
                    Leaf::Selectable => {
                        set_text_if(h, &crlf(&t.content.to_plain()));
                        set_paint_ink(h, ink);
                    }
                    _ => {
                        set_text_if(h, &t.content.to_plain());
                        set_paint_ink(h, ink);
                        invalidate(h);
                    }
                }
                set_tip(tip, &mut reg.tips, h, t.tooltip.as_deref());
            }
            (Node::One(h, leaf), Element::Button(b)) => {
                let h = *h;
                if *leaf == Leaf::IconButton {
                    set_text_if(h, b.hint.as_deref().unwrap_or(""));
                    repaint_with(
                        h,
                        Paint::Icon {
                            icon: b.icon.unwrap_or(wtm_toolkit::Icon::Close),
                            ground,
                            tint: b.tint,
                            small: true,
                        },
                    );
                } else {
                    set_text_if(h, &b.label.to_plain());
                    let style = if b.is_default {
                        BS_DEFPUSHBUTTON_
                    } else {
                        BS_PUSHBUTTON_
                    };
                    send(h, BM_SETSTYLE, style as usize, 1);
                }
                enable(h, b.enabled);
                set_tip(tip, &mut reg.tips, h, b.hint.as_deref());
                reg.bind(h, Bind::Press(b.on_press.clone()));
            }
            (Node::One(h, _), Element::Badge(b)) => {
                let h = *h;
                set_text_if(h, &b.text);
                repaint_with(
                    h,
                    Paint::Badge {
                        text: b.text.clone(),
                        hue: b.hue,
                        ground,
                    },
                );
                set_tip(tip, &mut reg.tips, h, Some(&b.tooltip));
            }
            (Node::One(h, _), Element::Spinner(s)) => {
                set_spinning(*h, s.spinning);
                set_tip(tip, &mut reg.tips, *h, s.tooltip.as_deref());
            }
            (Node::One(h, _), Element::Field(f)) => {
                let h = *h;
                set_text_if(h, &f.value);
                if self.cues.get(&key(h)) != Some(&f.placeholder) {
                    let cue = wide(&f.placeholder);
                    send(h, EM_SETCUEBANNER, 1, cue.as_ptr() as isize);
                    self.cues.insert(key(h), f.placeholder.clone());
                }
                enable_field(h, f.enabled);
                reg.bind(h, Bind::Text(f.on_change.clone()));
            }
            (Node::One(h, _), Element::Combo(c)) => {
                let h = *h;
                if self.options.get(&key(h)) != Some(&c.options) {
                    // Resetting the list empties the edit too; the text is
                    // put back below.
                    let text = text_of(h);
                    send(h, CB_RESETCONTENT, 0, 0);
                    for o in &c.options {
                        let wo = wide(o);
                        send(h, CB_ADDSTRING, 0, wo.as_ptr() as isize);
                    }
                    set_text(h, &text);
                    self.options.insert(key(h), c.options.clone());
                }
                set_text_if(h, &c.value);
                if self.cues.get(&key(h)) != Some(&c.placeholder) {
                    let cue = wide(&c.placeholder);
                    send(h, CB_SETCUEBANNER, 0, cue.as_ptr() as isize);
                    self.cues.insert(key(h), c.placeholder.clone());
                }
                enable_field(h, c.enabled);
                reg.bind(h, Bind::Combo(c.on_change.clone()));
            }
            (Node::One(h, _), Element::Choice(c)) => {
                let h = *h;
                if self.options.get(&key(h)) != Some(&c.options) {
                    send(h, CB_RESETCONTENT, 0, 0);
                    for o in &c.options {
                        let wo = wide(o);
                        send(h, CB_ADDSTRING, 0, wo.as_ptr() as isize);
                    }
                    self.options.insert(key(h), c.options.clone());
                }
                if send(h, CB_GETCURSEL, 0, 0) != c.selected as isize {
                    send(h, CB_SETCURSEL, c.selected, 0);
                }
                reg.bind(h, Bind::Choice(c.on_select.clone()));
            }
            (Node::Segments(v), Element::Segmented(s)) => {
                for (j, h) in v.iter().enumerate() {
                    set_text_if(*h, &s.options[j]);
                    let state = if j == s.selected { 1 } else { 0 };
                    if send(*h, BM_GETCHECK, 0, 0) != state as isize {
                        send(*h, BM_SETCHECK, state, 0);
                    }
                    reg.bind(*h, Bind::Segment(s.on_select.clone(), j));
                }
            }
            (Node::One(h, _), Element::TextBlock(text)) => {
                set_text_if(*h, &crlf(text));
            }
            _ => {}
        }
    }

    /// The natural size of the content at `width` (infinite: unwrapped).
    pub fn measure(&self, width: f64) -> Size {
        match &self.element {
            Some(e) => layout::measure(e, width, &metrics(), &mut Measurer),
            None => Size::default(),
        }
    }

    /// Put the pane at `r` (in its parent) and lay its controls out.
    pub fn place(&mut self, r: RECT) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        let Some(e) = &self.element else { return };
        let bounds = layout::Rect::new(
            0.0,
            0.0,
            (r.right - r.left) as f64,
            (r.bottom - r.top) as f64,
        );
        let l = layout::layout(e, bounds, &metrics(), &mut Measurer);
        let mut batch = Batch::new(&mut self.placed);
        let to_rect = |f: &layout::Rect| {
            rect(
                f.x.round() as i32,
                f.y.round() as i32,
                f.width.round() as i32,
                f.height.round() as i32,
            )
        };
        for (node, frame) in self.nodes.iter().zip(&l.frames) {
            match (node, frame) {
                (Node::One(h, leaf), Some(f)) => {
                    let mut r = to_rect(f);
                    if matches!(leaf, Leaf::Combo | Leaf::Choice) {
                        // A combo box's height includes its drop-down list.
                        r.bottom += look::px(200.0);
                    }
                    batch.place(*h, r);
                }
                (Node::One(h, _), None) => batch.hide(*h),
                (Node::Segments(v), Some(f)) => {
                    let r = to_rect(f);
                    let widths: Vec<i32> = v.iter().map(|h| segment_width(&text_of(*h))).collect();
                    let total: i32 = widths.iter().sum();
                    let mut x = r.left;
                    for (h, w) in v.iter().zip(&widths) {
                        let w = if total > 0 {
                            w * (r.right - r.left) / total
                        } else {
                            *w
                        };
                        batch.place(*h, rect(x, r.top, w, r.bottom - r.top));
                        x += w;
                    }
                }
                (Node::Segments(v), None) => v.iter().for_each(|h| batch.hide(*h)),
                _ => {}
            }
        }
        for (cap, frame) in self.captions.iter().zip(&l.captions) {
            if let Some(h) = cap {
                match frame {
                    Some(f) => batch.place(*h, to_rect(f)),
                    None => batch.hide(*h),
                }
            }
        }
        batch.apply();
    }
}

fn metrics() -> Metrics {
    let s = look::scale();
    Metrics {
        scale: s,
        form_column_gap: 10.0 * s,
        form_row_gap: 10.0 * s,
        form_hint_gap: 4.0 * s,
    }
}

fn segment_width(label: &str) -> i32 {
    look::text_size(Font::Body, label, None, false).0 + look::px(28.0)
}

/// Sizes of controls in the system font, as Windows' own layout guidelines
/// give them (a push button is 75 × 23 at 96 dpi).
struct Measurer;

impl Measure for Measurer {
    fn leaf(&mut self, e: &Element, max_width: f64) -> Size {
        let px = |v: f64| look::px(v) as f64;
        let max = (max_width.is_finite()).then(|| max_width.max(1.0) as i32);
        match e {
            Element::Text(t) => {
                let f = look::style_font(t.style, false);
                if look::has_marks_or_emphasis(&t.content) {
                    let (w, h) = look::rich_size(&t.content, t.style);
                    return Size::new(w as f64, h as f64);
                }
                let text = t.content.to_plain();
                let line = look::line_height(f) as f64;
                let (w, h) = look::text_size(f, &text, max, t.wrap);
                let mut h = h as f64;
                if t.lines > 0 {
                    h = h.min(line * t.lines as f64);
                }
                if text.is_empty() {
                    // An empty line keeps its height: notes and errors come
                    // and go under a field without moving it.
                    h = line;
                }
                Size::new(w as f64, h)
            }
            Element::Button(b) => match b.kind {
                ButtonKind::Icon => {
                    let (w, h) = look::icon_size(b.icon.unwrap_or(wtm_toolkit::Icon::Close), true);
                    Size::new(w as f64, h as f64)
                }
                kind => {
                    let f = button_font(kind);
                    let (tw, _) = look::text_size(f, &b.label.to_plain(), None, false);
                    let (min_w, height) = match kind {
                        ButtonKind::Push | ButtonKind::Pill => (75.0, 23.0),
                        _ => (0.0, 21.0),
                    };
                    Size::new((tw as f64 + px(20.0)).max(px(min_w)), px(height))
                }
            },
            Element::Badge(b) => {
                let (w, _) = look::text_size(Font::Badge, &b.text, None, false);
                Size::new(w as f64 + px(12.0), px(18.0))
            }
            Element::Spinner(_) => Size::new(px(16.0), px(16.0)),
            Element::Field(_) | Element::Combo(_) => Size::new(px(240.0), px(23.0)),
            Element::Choice(c) => {
                let widest = c
                    .options
                    .iter()
                    .map(|o| look::text_size(Font::Body, o, None, false).0)
                    .max()
                    .unwrap_or(0);
                Size::new(widest as f64 + px(40.0), px(23.0))
            }
            Element::Segmented(s) => {
                let w: i32 = s.options.iter().map(|o| segment_width(o)).sum();
                Size::new(w as f64, px(23.0))
            }
            Element::TextBlock(_) => Size::new(px(560.0), px(280.0)),
            _ => Size::default(),
        }
    }

    fn caption(&mut self, text: &str) -> Size {
        let (w, h) = look::text_size(Font::Body, text, None, false);
        Size::new(w as f64, h as f64)
    }
}
