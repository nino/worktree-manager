//! Stack layout for toolkits that have none of their own (Win32).
//!
//! A pure function from an [`Element`] tree and the sizes of its leaves to a
//! frame for every element. The toolkit measures what only it can (a label
//! in its font, a button with its bezel) through [`Measure`]; everything
//! else — spacing, gaps, flexible space, what grows, what gives way first
//! when a row is too narrow, the two columns of a form — is decided here, so
//! it is tested without a window system.
//!
//! Frames come back in [`Element::walk`] order, one per element, so a
//! backend that keeps one native control per element can zip the two.
//! Units are whatever the measurer returns (device pixels on Win32); the
//! spacings in the view are multiplied by [`Metrics::scale`] to match.

use crate::view::{Align, Axis, Element, Form, Shrink, Stack};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Self {
        Size { width, height }
    }
}

/// What only the toolkit knows: how big its controls are.
pub trait Measure {
    /// The natural size of a leaf (anything but a stack, a form, a gap or
    /// flexible space). `max_width` may be infinite; text that wraps wraps
    /// to it, anything else may ignore it.
    fn leaf(&mut self, element: &Element, max_width: f64) -> Size;

    /// The size of a form row's caption.
    fn caption(&mut self, text: &str) -> Size;
}

/// Spacing the view leaves to the toolkit, in the measurer's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Multiplies every spacing and gap the view gives (macOS points).
    pub scale: f64,
    /// Between a form's captions and its controls.
    pub form_column_gap: f64,
    /// Between a form's rows.
    pub form_row_gap: f64,
    /// Above a caption-less line of text in a form (a hint or a note), which
    /// belongs to the control above it.
    pub form_hint_gap: f64,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            scale: 1.0,
            form_column_gap: 8.0,
            form_row_gap: 10.0,
            form_hint_gap: 4.0,
        }
    }
}

/// Where everything goes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// One per element, in [`Element::walk`] order: `None` for an element
    /// that is hidden (or inside something hidden), and for gaps.
    pub frames: Vec<Option<Rect>>,
    /// One per form row, in the order the walk meets them: `None` for a row
    /// that is hidden or has no caption.
    pub captions: Vec<Option<Rect>>,
    /// The extent of what was placed, from the origin of the bounds.
    pub size: Size,
}

/// The natural size of `root` when it may be at most `max_width` wide.
pub fn measure(root: &Element, max_width: f64, metrics: &Metrics, m: &mut dyn Measure) -> Size {
    Engine { metrics, m }.measure(root, max_width)
}

/// Place `root` and everything in it within `bounds`.
pub fn layout(root: &Element, bounds: Rect, metrics: &Metrics, m: &mut dyn Measure) -> Layout {
    let mut out = Out {
        frames: Vec::with_capacity(count(root)),
        captions: Vec::new(),
    };
    let mut e = Engine { metrics, m };
    let size = e.measure(root, bounds.width);
    let frame = Rect::new(
        bounds.x,
        bounds.y,
        if fills_width(root) {
            bounds.width
        } else {
            size.width.min(bounds.width)
        },
        size.height.max(bounds.height),
    );
    e.place(root, Some(frame), &mut out);
    Layout {
        frames: out.frames,
        captions: out.captions,
        size,
    }
}

/// How many elements `walk` visits in `e`.
pub fn count(e: &Element) -> usize {
    let mut n = 0;
    e.walk(&mut |_| n += 1);
    n
}

// MARK: What stretches

fn hidden(e: &Element) -> bool {
    match e {
        Element::Stack(s) => s.hidden,
        Element::Text(t) => t.hidden,
        Element::Button(b) => b.hidden,
        _ => false,
    }
}

/// Takes the leftover length along its parent's axis.
fn grows(e: &Element, axis: Axis) -> bool {
    match e {
        Element::Flex => true,
        Element::Stack(s) => s.grow,
        Element::Text(t) => t.grow,
        // Fields stretch along a row, the way a text field's low hugging
        // priority makes it on macOS; nothing makes one taller.
        Element::Field(_) | Element::Combo(_) | Element::TextBlock(_) => axis == Axis::Horizontal,
        _ => false,
    }
}

/// Takes the whole width it is offered, across a column or in a form.
fn fills_width(e: &Element) -> bool {
    match e {
        Element::Field(_) | Element::Combo(_) | Element::TextBlock(_) | Element::Form(_) => true,
        Element::Text(t) => t.wrap,
        Element::Stack(s) => {
            s.grow
                || s.children
                    .iter()
                    .any(|c| !hidden(c) && (fills_width(c) || grows(c, s.axis)))
        }
        _ => false,
    }
}

/// When an element gives way in a row that is too narrow: `Shrink::First`,
/// then `Second`, then text that wraps (it loses nothing by it), then the
/// rest.
fn shrink_rank(e: &Element) -> u8 {
    let shrink = match e {
        Element::Stack(s) => s.shrink,
        Element::Text(t) => t.shrink,
        Element::Button(b) => b.shrink,
        _ => Shrink::Normal,
    };
    match shrink {
        Shrink::First => 0,
        Shrink::Second => 1,
        Shrink::Normal if matches!(e, Element::Text(t) if t.wrap) => 2,
        Shrink::Normal => 3,
    }
}

// MARK: Engine

struct Out {
    frames: Vec<Option<Rect>>,
    captions: Vec<Option<Rect>>,
}

struct Engine<'a> {
    metrics: &'a Metrics,
    m: &'a mut dyn Measure,
}

/// A stack child that takes part in layout, with the space before it.
struct Slot<'e> {
    element: &'e Element,
    before: f64,
}

impl Engine<'_> {
    fn measure(&mut self, e: &Element, max_width: f64) -> Size {
        match e {
            Element::Gap(_) | Element::Flex => Size::default(),
            Element::Stack(s) => match s.axis {
                Axis::Horizontal => {
                    let (widths, total) = self.row_widths(s, max_width);
                    let slots = self.slots(s);
                    let height = slots
                        .iter()
                        .zip(&widths)
                        .map(|(slot, w)| self.measure(slot.element, *w).height)
                        .fold(0.0, f64::max);
                    Size::new(total.min(max_width), height)
                }
                Axis::Vertical => {
                    let slots = self.slots(s);
                    let mut width: f64 = 0.0;
                    let mut height = 0.0;
                    for slot in &slots {
                        let size = self.measure(slot.element, max_width);
                        width = width.max(size.width);
                        height += slot.before + size.height;
                    }
                    Size::new(width.min(max_width), height)
                }
            },
            Element::Form(f) => self.measure_form(f, max_width),
            leaf => {
                let size = self.m.leaf(leaf, max_width);
                Size::new(size.width.min(max_width), size.height)
            }
        }
    }

    /// The visible children of a stack, each with the space before it: the
    /// stack's spacing, or the gap that precedes it in its place.
    fn slots<'e>(&self, s: &'e Stack) -> Vec<Slot<'e>> {
        let spacing = s.spacing * self.metrics.scale;
        let mut out: Vec<Slot> = Vec::new();
        let mut gap: Option<f64> = None;
        for c in &s.children {
            if hidden(c) {
                continue;
            }
            if let Element::Gap(g) = c {
                gap = Some(g * self.metrics.scale);
                continue;
            }
            let before = if out.is_empty() {
                0.0
            } else {
                gap.unwrap_or(spacing)
            };
            gap = None;
            out.push(Slot { element: c, before });
        }
        out
    }

    /// Each visible child's width in a row `available` wide, and the row's
    /// natural width (before anything grew or shrank).
    fn row_widths(&mut self, s: &Stack, available: f64) -> (Vec<f64>, f64) {
        let slots = self.slots(s);
        let mut widths: Vec<f64> = slots
            .iter()
            .map(|slot| self.measure(slot.element, available).width)
            .collect();
        let spacing: f64 = slots.iter().map(|s| s.before).sum();
        let total = spacing + widths.iter().sum::<f64>();
        if !available.is_finite() {
            return (widths, total);
        }
        let extra = available - total;
        if extra > 0.0 {
            let growers: Vec<usize> = (0..slots.len())
                .filter(|&i| grows(slots[i].element, Axis::Horizontal))
                .collect();
            if !growers.is_empty() {
                let share = extra / growers.len() as f64;
                for i in growers {
                    widths[i] += share;
                }
            }
        } else if extra < 0.0 {
            // Give way in order (see `shrink_rank`), each group in
            // proportion to its width.
            let mut deficit = -extra;
            for group in 0..=3 {
                if deficit <= 0.0 {
                    break;
                }
                let members: Vec<usize> = (0..slots.len())
                    .filter(|&i| shrink_rank(slots[i].element) == group)
                    .collect();
                let room: f64 = members.iter().map(|&i| widths[i]).sum();
                if room <= 0.0 {
                    continue;
                }
                let take = deficit.min(room);
                for &i in &members {
                    widths[i] -= take * widths[i] / room;
                }
                deficit -= take;
            }
        }
        (widths, total)
    }

    fn form_columns(&mut self, f: &Form) -> (f64, Vec<Size>) {
        let captions: Vec<Size> = f
            .rows
            .iter()
            .map(|r| {
                if r.hidden || r.caption.is_empty() {
                    Size::default()
                } else {
                    self.m.caption(&r.caption)
                }
            })
            .collect();
        let widest = captions.iter().map(|c| c.width).fold(0.0, f64::max);
        let start = if widest > 0.0 {
            widest + self.metrics.form_column_gap
        } else {
            0.0
        };
        (start, captions)
    }

    fn row_gap(&self, row: &crate::view::FormRow) -> f64 {
        if row.caption.is_empty() && matches!(row.content, Element::Text(_)) {
            self.metrics.form_hint_gap
        } else {
            self.metrics.form_row_gap
        }
    }

    fn content_width(&mut self, content: &Element, column: f64) -> f64 {
        if fills_width(content) {
            column
        } else {
            self.measure(content, column).width
        }
    }

    fn measure_form(&mut self, f: &Form, max_width: f64) -> Size {
        let (start, captions) = self.form_columns(f);
        let column = (max_width - start).max(0.0);
        let mut width: f64 = 0.0;
        let mut height = 0.0;
        let mut first = true;
        for (row, caption) in f.rows.iter().zip(&captions) {
            if row.hidden {
                continue;
            }
            if !first {
                height += self.row_gap(row);
            }
            first = false;
            let content = self.measure(&row.content, column);
            let w = if fills_width(&row.content) && column.is_finite() {
                column
            } else {
                content.width
            };
            width = width.max(start + w);
            height += content.height.max(caption.height);
        }
        Size::new(width.min(max_width), height)
    }

    // MARK: Placing

    fn skip(&mut self, e: &Element, out: &mut Out) {
        e.walk(&mut |x| {
            out.frames.push(None);
            if let Element::Form(f) = x {
                out.captions.extend(f.rows.iter().map(|_| None));
            }
        });
    }

    fn place(&mut self, e: &Element, frame: Option<Rect>, out: &mut Out) {
        let Some(frame) = frame.filter(|_| !hidden(e)) else {
            self.skip(e, out);
            return;
        };
        if let Element::Gap(_) = e {
            out.frames.push(None);
            return;
        }
        out.frames.push(Some(frame));
        match e {
            Element::Stack(s) => self.place_stack(s, frame, out),
            Element::Form(f) => self.place_form(f, frame, out),
            _ => {}
        }
    }

    fn place_stack(&mut self, s: &Stack, frame: Rect, out: &mut Out) {
        let slots = self.slots(s);
        let mut placed: Vec<Option<Rect>> = vec![None; s.children.len()];
        let index = |slot: &Slot| {
            s.children
                .iter()
                .position(|c| std::ptr::eq(c, slot.element))
                .expect("a slot is one of the stack's children")
        };
        match s.axis {
            Axis::Horizontal => {
                let (widths, _) = self.row_widths(s, frame.width);
                let mut x = frame.x;
                for (slot, w) in slots.iter().zip(widths) {
                    x += slot.before;
                    let natural = self.measure(slot.element, w).height;
                    let (y, h) = match s.align {
                        Align::Fill => (frame.y, frame.height),
                        Align::Start => (frame.y, natural),
                        Align::Center | Align::Baseline => {
                            (frame.y + ((frame.height - natural) / 2.0).max(0.0), natural)
                        }
                    };
                    placed[index(slot)] = Some(Rect::new(x, y, w.max(0.0), h));
                    x += w.max(0.0);
                }
            }
            Axis::Vertical => {
                let mut sizes = Vec::with_capacity(slots.len());
                let mut total = 0.0;
                for slot in &slots {
                    let w = if s.align == Align::Fill || fills_width(slot.element) {
                        frame.width
                    } else {
                        self.measure(slot.element, frame.width).width
                    };
                    let h = self.measure(slot.element, w).height;
                    total += slot.before + h;
                    sizes.push((w, h));
                }
                let extra = frame.height - total;
                let growers = slots
                    .iter()
                    .filter(|s| grows(s.element, Axis::Vertical))
                    .count();
                let share = if extra > 0.0 && growers > 0 {
                    extra / growers as f64
                } else {
                    0.0
                };
                let mut y = frame.y;
                for (slot, (w, mut h)) in slots.iter().zip(sizes) {
                    y += slot.before;
                    if grows(slot.element, Axis::Vertical) {
                        h += share;
                    }
                    let x = match s.align {
                        Align::Center => frame.x + ((frame.width - w) / 2.0).max(0.0),
                        _ => frame.x,
                    };
                    placed[index(slot)] = Some(Rect::new(x, y, w, h));
                    y += h;
                }
            }
        }
        for (c, rect) in s.children.iter().zip(placed) {
            self.place(c, rect, out);
        }
    }

    fn place_form(&mut self, f: &Form, frame: Rect, out: &mut Out) {
        let (start, captions) = self.form_columns(f);
        let column = (frame.width - start).max(0.0);
        let mut y = frame.y;
        let mut first = true;
        let mut contents = Vec::with_capacity(f.rows.len());
        for (row, caption) in f.rows.iter().zip(&captions) {
            if row.hidden {
                out.captions.push(None);
                contents.push(None);
                continue;
            }
            if !first {
                y += self.row_gap(row);
            }
            first = false;
            let w = self.content_width(&row.content, column);
            let h = self.measure(&row.content, w).height;
            let height = h.max(caption.height);
            // A caption sits level with a control, and with the first line
            // of text.
            let caption_y = match row.content {
                Element::Text(_) | Element::TextBlock(_) => y,
                _ => y + ((h - caption.height) / 2.0).max(0.0),
            };
            out.captions.push((!row.caption.is_empty()).then(|| {
                Rect::new(
                    frame.x + start - self.metrics.form_column_gap - caption.width,
                    caption_y,
                    caption.width,
                    caption.height,
                )
            }));
            contents.push(Some(Rect::new(frame.x + start, y, w, h)));
            y += height;
        }
        for (row, rect) in f.rows.iter().zip(contents) {
            self.place(&row.content, rect, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{Button, ButtonKind, Field, FormRow, Handler, Text};

    /// Every leaf is 10 high; text is 6 per character and wraps to the
    /// width it is given; a button is its label plus 20; a field is 100.
    struct Fixed;

    impl Measure for Fixed {
        fn leaf(&mut self, e: &Element, max_width: f64) -> Size {
            match e {
                Element::Text(t) => {
                    let full = 6.0 * t.content.to_plain().chars().count() as f64;
                    if t.wrap && full > max_width {
                        let lines = (full / max_width).ceil();
                        Size::new(max_width, 10.0 * lines)
                    } else {
                        Size::new(full, 10.0)
                    }
                }
                Element::Button(b) => {
                    Size::new(6.0 * b.label.to_plain().chars().count() as f64 + 20.0, 10.0)
                }
                Element::Field(_) => Size::new(100.0, 10.0),
                _ => Size::new(10.0, 10.0),
            }
        }

        fn caption(&mut self, text: &str) -> Size {
            Size::new(6.0 * text.chars().count() as f64, 8.0)
        }
    }

    fn text(s: &str) -> Element {
        Text::new(s).into()
    }

    fn button(label: &str) -> Element {
        Button::new("b", ButtonKind::Push, label, Handler::none()).into()
    }

    fn field() -> Element {
        Field {
            id: "f",
            value: String::new(),
            placeholder: String::new(),
            enabled: true,
            on_change: Handler::none(),
        }
        .into()
    }

    fn run(e: &Element, bounds: Rect) -> Layout {
        layout(e, bounds, &Metrics::default(), &mut Fixed)
    }

    fn frame(l: &Layout, i: usize) -> Rect {
        l.frames[i].unwrap_or_else(|| panic!("element {i} has no frame"))
    }

    #[test]
    fn one_frame_per_element_in_walk_order() {
        let e: Element = Stack::column(0.0)
            .child(Stack::row(0.0).child(text("a")).child(text("b")))
            .child(Form::default().row("x", field()))
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 300.0, 100.0));
        assert_eq!(l.frames.len(), count(&e));
        assert_eq!(l.frames.len(), 6);
        assert_eq!(l.captions.len(), 1);
    }

    #[test]
    fn a_row_places_children_with_its_spacing_and_gaps() {
        let e: Element = Stack::row(4.0)
            .align(Align::Start)
            .child(text("ab"))
            .child(text("c"))
            .child(Element::Gap(10.0))
            .child(text("d"))
            .into();
        let l = run(&e, Rect::new(5.0, 7.0, 300.0, 10.0));
        assert_eq!(frame(&l, 1), Rect::new(5.0, 7.0, 12.0, 10.0));
        assert_eq!(frame(&l, 2), Rect::new(21.0, 7.0, 6.0, 10.0));
        assert_eq!(l.frames[3], None, "gaps take no frame");
        // The gap replaces the spacing rather than adding to it.
        assert_eq!(frame(&l, 4).x, 37.0);
        assert_eq!(l.size, Size::new(38.0, 10.0));
    }

    #[test]
    fn spacing_scales_and_hidden_children_take_no_room() {
        let e: Element = Stack::row(4.0)
            .child(text("a"))
            .child(Text::new("gone").hidden(true))
            .child(text("b"))
            .into();
        let metrics = Metrics {
            scale: 2.0,
            ..Metrics::default()
        };
        let l = layout(&e, Rect::new(0.0, 0.0, 100.0, 10.0), &metrics, &mut Fixed);
        assert_eq!(l.frames[2], None);
        assert_eq!(frame(&l, 3).x, 6.0 + 8.0);
    }

    #[test]
    fn flex_pushes_what_follows_to_the_end() {
        let e: Element = Stack::row(0.0)
            .child(text("a"))
            .child(Element::Flex)
            .child(button("ok"))
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 200.0, 10.0));
        assert_eq!(frame(&l, 3).right(), 200.0);
        assert_eq!(frame(&l, 2), Rect::new(6.0, 5.0, 200.0 - 6.0 - 32.0, 0.0));
    }

    #[test]
    fn growers_share_the_leftover_and_fields_stretch() {
        let e: Element = Stack::row(0.0)
            .child(Text::new("a").grow())
            .child(field())
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 306.0, 10.0));
        // 106 natural, 200 left over, 100 each.
        assert_eq!(frame(&l, 1).width, 106.0);
        assert_eq!(frame(&l, 2).width, 200.0);
    }

    #[test]
    fn a_narrow_row_shrinks_first_then_second_then_the_rest() {
        let e: Element = Stack::row(0.0)
            .child(text("aaaaaaaaaa")) // 60
            .child(Text::new("bbbbbbbbbb").shrink(Shrink::Second)) // 60
            .child(Text::new("cccccccccc").shrink(Shrink::First)) // 60
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 130.0, 10.0));
        assert_eq!(frame(&l, 3).width, 10.0, "first gives way first");
        assert_eq!(frame(&l, 2).width, 60.0);
        assert_eq!(frame(&l, 1).width, 60.0);

        let l = run(&e, Rect::new(0.0, 0.0, 90.0, 10.0));
        assert_eq!(frame(&l, 3).width, 0.0);
        assert_eq!(frame(&l, 2).width, 30.0, "then second");
        assert_eq!(frame(&l, 1).width, 60.0, "the rest keep theirs");
    }

    #[test]
    fn rows_centre_their_children_across() {
        let tall = Stack::column(0.0).child(text("a")).child(text("b"));
        let e: Element = Stack::row(0.0).child(tall).child(text("c")).into();
        let l = run(&e, Rect::new(0.0, 0.0, 100.0, 20.0));
        assert_eq!(frame(&l, 4), Rect::new(6.0, 5.0, 6.0, 10.0));
    }

    #[test]
    fn wrapping_text_in_a_row_wraps_to_what_is_left() {
        let e: Element = Stack::row(0.0)
            .align(Align::Start)
            .child(Text::new("x".repeat(30)).wrap().grow())
            .child(button("ok"))
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 122.0, 10.0));
        // 180 + 32 asked for, 122 available: the text gives up 90.
        assert_eq!(frame(&l, 1).width, 90.0);
        assert_eq!(frame(&l, 1).height, 20.0, "two lines at 90 wide");
        assert_eq!(l.size.height, 20.0);
    }

    #[test]
    fn a_centred_column_centres_and_fields_fill_it() {
        let e: Element = Stack::column(10.0)
            .align(Align::Center)
            .child(text("abcd"))
            .child(field())
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(frame(&l, 1), Rect::new(88.0, 0.0, 24.0, 10.0));
        assert_eq!(frame(&l, 2), Rect::new(0.0, 20.0, 200.0, 10.0));
    }

    #[test]
    fn a_column_gives_its_extra_height_to_what_grows() {
        let e: Element = Stack::column(0.0)
            .child(text("a"))
            .child(Element::Flex)
            .child(text("b"))
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 50.0, 100.0));
        assert_eq!(frame(&l, 3).y, 90.0);
    }

    #[test]
    fn forms_line_captions_up_in_their_own_column() {
        let form = Form::default()
            .row("Name:", field())
            .row("Longer name:", field())
            .hint("a hint");
        let e: Element = form.into();
        let l = run(&e, Rect::new(0.0, 0.0, 300.0, 0.0));
        let start = 6.0 * 12.0 + 8.0;
        let (a, b, hint) = (frame(&l, 1), frame(&l, 2), frame(&l, 3));
        assert_eq!(a, Rect::new(start, 0.0, 300.0 - start, 10.0));
        assert_eq!(b.y, 20.0);
        assert_eq!(hint.y, 34.0, "a hint sits close under its control");
        assert_eq!(hint.x, start, "and lines up with the controls");
        // Captions end at the column's edge and centre on their control.
        let c = l.captions[0].unwrap();
        assert_eq!(c.right(), start - 8.0);
        assert_eq!(c.y, 1.0);
        assert_eq!(l.captions[2], None, "a hint has no caption");
        assert_eq!(l.size.height, 44.0);
    }

    #[test]
    fn a_hidden_form_row_takes_no_room() {
        let form = Form {
            rows: vec![
                FormRow {
                    caption: "A:".into(),
                    content: field(),
                    hidden: true,
                },
                FormRow {
                    caption: "B:".into(),
                    content: field(),
                    hidden: false,
                },
            ],
        };
        let l = run(&form.into(), Rect::new(0.0, 0.0, 200.0, 0.0));
        assert_eq!(l.frames[1], None);
        assert_eq!(l.captions[0], None);
        assert_eq!(frame(&l, 2).y, 0.0);
    }

    #[test]
    fn hidden_stacks_hide_everything_inside() {
        let e: Element = Stack::column(0.0)
            .child(
                Stack::row(0.0)
                    .hidden(true)
                    .child(text("a"))
                    .child(Form::default().row("x", field())),
            )
            .child(text("b"))
            .into();
        let l = run(&e, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(&l.frames[1..5], &[None, None, None, None]);
        assert_eq!(l.captions, vec![None]);
        assert_eq!(frame(&l, 5).y, 0.0);
    }

    #[test]
    fn natural_size_is_capped_by_the_width_offered() {
        let e: Element = Stack::row(0.0).child(text(&"x".repeat(100))).into();
        assert_eq!(
            measure(&e, 50.0, &Metrics::default(), &mut Fixed),
            Size::new(50.0, 10.0)
        );
        assert_eq!(
            measure(&e, f64::INFINITY, &Metrics::default(), &mut Fixed).width,
            600.0
        );
    }
}
