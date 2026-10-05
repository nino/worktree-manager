//! Generic [`Element`]s as widgets: the notice bar, the empty state, dialog
//! bodies and the Settings window. Each element becomes a [`Node`] that keeps
//! its widget and handler slots, and a new element of the same shape patches
//! the node in place, so a field keeps the keyboard and its cursor while the
//! dialog around it re-renders on every keystroke.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use wtm_toolkit::{
    Align, Axis, Badge, Button, ButtonKind, Choice, Combo, Element, Field, Id, Segmented, Shrink,
    Spinner, Stack, Text, TextAlign, Tint,
};

use crate::draw;
use crate::rich::{LabelOpts, RichLabel};
use crate::style;
use crate::util::{
    fire, has_keyboard, put, set_hint, set_sensitive, set_tooltip, show, slot, spin, Slot,
};

pub struct Node {
    /// `None` for a gap, which only spaces its neighbours.
    pub widget: Option<gtk::Widget>,
    kind: Kind,
}

enum Kind {
    Stack {
        b: gtk::Box,
        children: Vec<Node>,
        last: Stack,
    },
    Text {
        rich: RichLabel,
        last: Text,
    },
    Button(ButtonNode),
    Badge {
        label: gtk::Label,
        last: Badge,
    },
    Spinner {
        s: gtk::Spinner,
    },
    Field(FieldNode),
    Combo(ComboNode),
    Segmented(SegmentedNode),
    Choice(ChoiceNode),
    TextBlock {
        view: gtk::TextView,
        text: String,
    },
    Form {
        grid: gtk::Grid,
        rows: Vec<FormRowNode>,
    },
    Gap(f64),
    Flex,
}

impl Node {
    /// Build `el` for a parent laid out along `axis`.
    pub fn build(el: &Element, axis: Axis) -> Node {
        let (widget, kind) = match el {
            Element::Stack(s) => {
                let b = gtk::Box::new(orientation(s.axis), 0);
                let children: Vec<Node> =
                    s.children.iter().map(|c| Node::build(c, s.axis)).collect();
                for c in &children {
                    if let Some(w) = &c.widget {
                        b.append(w);
                    }
                }
                let last = Stack {
                    children: Vec::new(),
                    ..s.clone()
                };
                let node = Kind::Stack {
                    b: b.clone(),
                    children,
                    last,
                };
                (Some(b.upcast()), node)
            }
            Element::Text(t) => {
                let rich = RichLabel::new(&t.content, text_opts(t));
                let w = rich.root.clone().upcast::<gtk::Widget>();
                style_text(&w, None, t);
                (
                    Some(w),
                    Kind::Text {
                        rich,
                        last: t.clone(),
                    },
                )
            }
            Element::Button(b) => {
                let node = ButtonNode::new(b);
                (Some(node.button.clone().upcast()), Kind::Button(node))
            }
            Element::Badge(b) => {
                let label = badge(b);
                (
                    Some(label.clone().upcast()),
                    Kind::Badge {
                        label,
                        last: b.clone(),
                    },
                )
            }
            Element::Spinner(s) => {
                let sp = gtk::Spinner::new();
                patch_spinner(&sp, s);
                (Some(sp.clone().upcast()), Kind::Spinner { s: sp })
            }
            Element::Field(f) => {
                let node = FieldNode::new(f);
                (Some(node.entry.clone().upcast()), Kind::Field(node))
            }
            Element::Combo(c) => {
                let node = ComboNode::new(c);
                (Some(node.root.clone().upcast()), Kind::Combo(node))
            }
            Element::Segmented(s) => {
                let node = SegmentedNode::new(s);
                (Some(node.root.clone().upcast()), Kind::Segmented(node))
            }
            Element::Choice(c) => {
                let node = ChoiceNode::new(c);
                (Some(node.dropdown.clone().upcast()), Kind::Choice(node))
            }
            Element::TextBlock(text) => {
                let view = gtk::TextView::new();
                view.set_editable(false);
                view.set_cursor_visible(false);
                view.set_monospace(true);
                view.add_css_class("wtm-textblock");
                view.set_left_margin(8);
                view.set_right_margin(8);
                view.set_top_margin(6);
                view.set_bottom_margin(6);
                view.buffer().set_text(text);
                let scroll = gtk::ScrolledWindow::new();
                scroll.set_child(Some(&view));
                scroll.set_min_content_height(220);
                scroll.set_min_content_width(480);
                scroll.set_has_frame(true);
                scroll.set_vexpand(true);
                (
                    Some(scroll.upcast()),
                    Kind::TextBlock {
                        view,
                        text: text.clone(),
                    },
                )
            }
            Element::Form(f) => {
                let grid = gtk::Grid::new();
                grid.set_column_spacing(10);
                grid.set_row_spacing(6);
                let rows = f
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| FormRowNode::new(&grid, i as i32, r))
                    .collect();
                (Some(grid.clone().upcast()), Kind::Form { grid, rows })
            }
            Element::Gap(g) => (None, Kind::Gap(*g)),
            Element::Flex => {
                let w = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                (Some(w.upcast()), Kind::Flex)
            }
        };
        let node = Node { widget, kind };
        node.layout(el, axis);
        if let Kind::Stack { .. } = node.kind {
            node.space();
        }
        node
    }

    /// Bring this node in line with `el`. `false` when it has another shape
    /// and must be rebuilt by the caller.
    pub fn patch(&mut self, el: &Element, axis: Axis) -> bool {
        let ok = match (&mut self.kind, el) {
            (Kind::Stack { b, children, last }, Element::Stack(s)) => {
                let same = children.len() == s.children.len()
                    && children
                        .iter()
                        .zip(&s.children)
                        .all(|(n, e)| n.same_kind(e));
                if !same {
                    return false;
                }
                if last.axis != s.axis {
                    return false;
                }
                let mut prev: Option<gtk::Widget> = None;
                for (child, e) in children.iter_mut().zip(&s.children) {
                    if !child.patch(e, s.axis) {
                        let fresh = Node::build(e, s.axis);
                        if let Some(w) = &fresh.widget {
                            b.insert_child_after(w, prev.as_ref());
                        }
                        if let Some(old) = &child.widget {
                            b.remove(old);
                        }
                        *child = fresh;
                    }
                    if let Some(w) = &child.widget {
                        prev = Some(w.clone());
                    }
                }
                *last = Stack {
                    children: Vec::new(),
                    ..s.clone()
                };
                true
            }
            (Kind::Text { rich, last }, Element::Text(t)) => {
                if last != t {
                    rich.set(&t.content);
                    rich.set_opts(text_opts(t));
                    let w = rich.root.clone().upcast::<gtk::Widget>();
                    style_text(&w, Some(last), t);
                    *last = t.clone();
                }
                true
            }
            (Kind::Button(n), Element::Button(b)) => n.patch(b),
            (Kind::Badge { label, last }, Element::Badge(b)) => {
                patch_badge(label, Some(last), b);
                *last = b.clone();
                true
            }
            (Kind::Spinner { s }, Element::Spinner(sp)) => {
                patch_spinner(s, sp);
                true
            }
            (Kind::Field(n), Element::Field(f)) => n.patch(f),
            (Kind::Combo(n), Element::Combo(c)) => n.patch(c),
            (Kind::Segmented(n), Element::Segmented(s)) => n.patch(s),
            (Kind::Choice(n), Element::Choice(c)) => n.patch(c),
            (Kind::TextBlock { view, text }, Element::TextBlock(t)) => {
                if text != t {
                    view.buffer().set_text(t);
                    *text = t.clone();
                }
                true
            }
            (Kind::Form { grid, rows }, Element::Form(f)) => {
                let same = rows.len() == f.rows.len()
                    && rows
                        .iter()
                        .zip(&f.rows)
                        .all(|(n, r)| n.caption.is_some() == !r.caption.is_empty());
                if !same {
                    return false;
                }
                for (i, (n, r)) in rows.iter_mut().zip(&f.rows).enumerate() {
                    n.patch(grid, i as i32, r);
                }
                true
            }
            (Kind::Gap(g), Element::Gap(x)) => {
                *g = *x;
                true
            }
            (Kind::Flex, Element::Flex) => true,
            _ => false,
        };
        if ok {
            self.layout(el, axis);
            if let Kind::Stack { .. } = self.kind {
                self.space();
            }
        }
        ok
    }

    fn same_kind(&self, el: &Element) -> bool {
        match &self.kind {
            Kind::Stack { .. } => matches!(el, Element::Stack(_)),
            Kind::Text { .. } => matches!(el, Element::Text(_)),
            Kind::Button(_) => matches!(el, Element::Button(_)),
            Kind::Badge { .. } => matches!(el, Element::Badge(_)),
            Kind::Spinner { .. } => matches!(el, Element::Spinner(_)),
            Kind::Field(_) => matches!(el, Element::Field(_)),
            Kind::Combo(_) => matches!(el, Element::Combo(_)),
            Kind::Segmented(_) => matches!(el, Element::Segmented(_)),
            Kind::Choice(_) => matches!(el, Element::Choice(_)),
            Kind::TextBlock { .. } => matches!(el, Element::TextBlock(_)),
            Kind::Form { .. } => matches!(el, Element::Form(_)),
            Kind::Gap(_) => matches!(el, Element::Gap(_)),
            Kind::Flex => matches!(el, Element::Flex),
        }
    }

    /// Grow, hide and alignment, which belong to the element's place in its
    /// parent.
    fn layout(&self, el: &Element, axis: Axis) {
        let Some(w) = &self.widget else { return };
        let (grow, hidden) = match el {
            Element::Stack(s) => (s.grow, s.hidden),
            Element::Text(t) => (t.grow, t.hidden),
            Element::Button(b) => (false, b.hidden),
            Element::Flex => (true, false),
            Element::Field(_) | Element::Combo(_) => (axis == Axis::Horizontal, false),
            _ => (false, false),
        };
        match axis {
            Axis::Horizontal => w.set_hexpand(grow),
            Axis::Vertical => w.set_vexpand(grow),
        }
        show(w, !hidden);
        if let (Kind::Stack { last, children, .. }, Element::Stack(_)) = (&self.kind, el) {
            for c in children {
                if let Some(cw) = &c.widget {
                    align_child(cw, last.axis, last.align);
                }
            }
        }
    }

    /// A stack's spacing, as margins: the gap after an `Element::Gap` is its
    /// own, every other one the stack's.
    fn space(&self) {
        let Kind::Stack { children, last, .. } = &self.kind else {
            return;
        };
        let mut first = true;
        let mut gap: Option<f64> = None;
        for c in children {
            match (&c.kind, &c.widget) {
                (Kind::Gap(g), _) => gap = Some(*g),
                (_, Some(w)) => {
                    let m = if first {
                        0.0
                    } else {
                        gap.unwrap_or(last.spacing)
                    };
                    let m = m.round() as i32;
                    match last.axis {
                        Axis::Horizontal => {
                            if w.margin_start() != m {
                                w.set_margin_start(m)
                            }
                        }
                        Axis::Vertical => {
                            if w.margin_top() != m {
                                w.set_margin_top(m)
                            }
                        }
                    }
                    first = false;
                    gap = None;
                }
                _ => {}
            }
        }
    }

    /// The widget for the element with `id` (a field, a button…), for focus.
    pub fn find(&self, id: &str) -> Option<gtk::Widget> {
        match &self.kind {
            Kind::Stack { children, .. } => children.iter().find_map(|c| c.find(id)),
            Kind::Form { rows, .. } => rows.iter().find_map(|r| r.content.find(id)),
            Kind::Field(n) if n.id == id => Some(n.entry.clone().upcast()),
            Kind::Combo(n) if n.id == id => Some(n.entry.clone().upcast()),
            Kind::Segmented(n) if n.id == id => n.buttons.first().map(|b| b.clone().upcast()),
            Kind::Choice(n) if n.id == id => Some(n.dropdown.clone().upcast()),
            Kind::Button(n) if n.id == id => Some(n.button.clone().upcast()),
            Kind::Text { rich, last } if last.id == Some(id) => Some(rich.root.clone().upcast()),
            _ => None,
        }
    }
}

fn orientation(axis: Axis) -> gtk::Orientation {
    match axis {
        Axis::Horizontal => gtk::Orientation::Horizontal,
        Axis::Vertical => gtk::Orientation::Vertical,
    }
}

fn align_child(w: &gtk::Widget, axis: Axis, align: Align) {
    let a = match align {
        Align::Start => gtk::Align::Start,
        Align::Center => gtk::Align::Center,
        Align::Baseline if axis == Axis::Horizontal => gtk::Align::BaselineCenter,
        Align::Baseline => gtk::Align::Start,
        Align::Fill => gtk::Align::Fill,
    };
    match axis {
        Axis::Horizontal => w.set_valign(a),
        Axis::Vertical => {
            // A child that grows across a column fills it whatever the
            // column's alignment.
            if !w.hexpands() {
                w.set_halign(a)
            } else {
                w.set_halign(gtk::Align::Fill)
            }
        }
    }
}

// MARK: Text

fn text_opts(t: &Text) -> LabelOpts {
    LabelOpts {
        ellipsize: t.shrink != Shrink::Normal || (t.lines == 1 && t.wrap),
        wrap: t.wrap,
        lines: t.lines as i32,
        selectable: t.selectable,
        xalign: match t.align {
            TextAlign::Leading => 0.0,
            TextAlign::Trailing => 1.0,
            TextAlign::Center => 0.5,
        },
    }
}

fn text_classes(t: &Text) -> Vec<&'static str> {
    style::text_class(t.style)
        .into_iter()
        .chain(style::ink_class(t.ink))
        .collect()
}

pub fn style_text(w: &gtk::Widget, old: Option<&Text>, t: &Text) {
    let old_classes = old.map(text_classes).unwrap_or_default();
    style::swap_classes(w, &old_classes, &text_classes(t));
    set_tooltip(w, t.tooltip.as_deref());
    if t.wrap {
        // A wrapping label asks for its whole length on one line otherwise,
        // and a dialog would grow to fit it.
        let mut c = w.first_child();
        while let Some(l) = c {
            if let Some(l) = l.downcast_ref::<gtk::Label>() {
                l.set_max_width_chars(56);
                l.set_width_chars(20);
            }
            c = l.next_sibling();
        }
    }
    if t.align == TextAlign::Center {
        w.set_halign(gtk::Align::Center);
    }
}

// MARK: Badges and spinners

pub fn badge(b: &Badge) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("wtm-badge");
    l.set_valign(gtk::Align::Center);
    patch_badge(&l, None, b);
    l
}

pub fn patch_badge(l: &gtk::Label, old: Option<&Badge>, b: &Badge) {
    if old == Some(b) {
        return;
    }
    if l.text() != b.text {
        l.set_text(&b.text);
    }
    let old_classes: Vec<&str> = old
        .map(|o| vec![style::hue_class(o.hue), style::emphasis_class(o.emphasis)])
        .unwrap_or_default();
    style::swap_classes(
        l,
        &old_classes,
        &[style::hue_class(b.hue), style::emphasis_class(b.emphasis)],
    );
    set_tooltip(l, Some(&b.tooltip));
}

pub fn patch_spinner(s: &gtk::Spinner, sp: &Spinner) {
    spin(s, sp.spinning);
    set_tooltip(s, sp.tooltip.as_deref().filter(|_| sp.spinning));
}

// MARK: Buttons

pub struct ButtonNode {
    pub id: Id,
    pub button: gtk::Button,
    kind: ButtonKind,
    icon: Option<wtm_toolkit::Icon>,
    label: Option<RichLabel>,
    is_default: bool,
    on_press: Slot<()>,
    last: Button,
}

impl ButtonNode {
    pub fn new(b: &Button) -> ButtonNode {
        let button = gtk::Button::new();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        if let Some(icon) = b.icon {
            content.append(&draw::icon(
                icon,
                if b.kind == ButtonKind::Icon { 14 } else { 16 },
            ));
        }
        let label = (b.kind != ButtonKind::Icon).then(|| {
            let l = RichLabel::new(&b.label, LabelOpts::default());
            content.append(&l.root);
            l
        });
        if b.kind == ButtonKind::Pill {
            content.append(&draw::chevrons());
        }
        button.set_child(Some(&content));
        match b.kind {
            ButtonKind::Push => {}
            ButtonKind::Small => button.add_css_class("wtm-small"),
            ButtonKind::Accessory => {
                button.add_css_class("flat");
                button.add_css_class("wtm-accessory");
            }
            ButtonKind::Icon => {
                button.add_css_class("flat");
                button.add_css_class("wtm-icon");
            }
            ButtonKind::Pill => button.add_css_class("wtm-pill"),
        }
        button.set_valign(gtk::Align::Center);
        let on_press = slot(&b.on_press);
        crate::util::on_click(&button, &on_press);
        let mut node = ButtonNode {
            id: b.id,
            button,
            kind: b.kind,
            icon: b.icon,
            label,
            is_default: b.is_default,
            on_press,
            last: b.clone(),
        };
        node.apply(None, b);
        node
    }

    fn patch(&mut self, b: &Button) -> bool {
        if b.kind != self.kind || b.icon != self.icon || b.id != self.id {
            return false;
        }
        put(&self.on_press, &b.on_press);
        if &self.last != b {
            let last = self.last.clone();
            self.apply(Some(&last), b);
            self.last = b.clone();
        }
        true
    }

    fn apply(&mut self, old: Option<&Button>, b: &Button) {
        if let Some(l) = &self.label {
            l.set(&b.label);
        }
        set_sensitive(&self.button, b.enabled);
        show(&self.button, !b.hidden);
        if let Some(hint) = &b.hint {
            set_hint(&self.button, hint, b.kind == ButtonKind::Icon);
        }
        if let Some(name) = &b.a11y_label {
            self.button
                .update_property(&[gtk::accessible::Property::Label(name)]);
        }
        let tint = |t: Tint, kind: ButtonKind| match (t, kind) {
            (Tint::Danger, ButtonKind::Icon) => Some("danger"),
            (Tint::Danger, _) => Some("destructive-action"),
            _ => None,
        };
        let old_classes: Vec<&str> = old
            .map(|o| {
                tint(o.tint, o.kind)
                    .into_iter()
                    .chain(o.is_default.then_some("suggested-action"))
                    .collect()
            })
            .unwrap_or_default();
        let new: Vec<&str> = tint(b.tint, b.kind)
            .into_iter()
            .chain(b.is_default.then_some("suggested-action"))
            .collect();
        style::swap_classes(&self.button, &old_classes, &new);
        self.is_default = b.is_default;
    }
}

// MARK: Fields

pub struct FieldNode {
    pub id: Id,
    pub entry: gtk::Entry,
    on_change: Slot<String>,
}

impl FieldNode {
    fn new(f: &Field) -> FieldNode {
        let entry = gtk::Entry::new();
        entry.set_width_chars(28);
        let on_change = slot(&f.on_change);
        let s = on_change.clone();
        entry.connect_changed(move |e| fire(&s, e.text().to_string()));
        let mut n = FieldNode {
            id: f.id,
            entry,
            on_change,
        };
        n.patch(f);
        n
    }

    fn patch(&mut self, f: &Field) -> bool {
        if f.id != self.id {
            return false;
        }
        put(&self.on_change, &f.on_change);
        set_entry(&self.entry, &f.value, &f.placeholder, f.enabled);
        true
    }
}

/// The field rules: text is written only when it differs and the user is
/// not typing in it (the view can lag a keystroke behind); a field with the
/// keyboard is never disabled.
pub fn set_entry(entry: &gtk::Entry, value: &str, placeholder: &str, enabled: bool) {
    let typing = has_keyboard(entry);
    if !typing && entry.text() != value {
        entry.set_text(value);
    }
    if entry.placeholder_text().as_deref() != Some(placeholder) {
        entry.set_placeholder_text(Some(placeholder));
    }
    set_sensitive(entry, enabled || typing);
}

pub struct ComboNode {
    pub id: Id,
    pub root: gtk::Box,
    pub entry: gtk::Entry,
    menu: gtk::MenuButton,
    list: gtk::ListBox,
    options: Rc<RefCell<Vec<String>>>,
    on_change: Slot<String>,
}

impl ComboNode {
    fn new(c: &Combo) -> ComboNode {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("linked");
        let entry = gtk::Entry::new();
        entry.set_hexpand(true);
        entry.set_width_chars(24);
        let menu = gtk::MenuButton::new();
        menu.set_icon_name("pan-down-symbolic");
        set_hint(&menu, "Suggestions", true);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&list));
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(260);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&scroll));
        menu.set_popover(Some(&popover));
        root.append(&entry);
        root.append(&menu);
        let on_change = slot(&c.on_change);
        let s = on_change.clone();
        entry.connect_changed(move |e| fire(&s, e.text().to_string()));
        let options: Rc<RefCell<Vec<String>>> = Rc::default();
        {
            let (entry, options, popover) = (entry.clone(), options.clone(), popover.clone());
            // Picking writes the entry, whose change is reported like typing.
            list.connect_row_activated(move |_, row| {
                let i = row.index();
                let chosen = options.borrow().get(i as usize).cloned();
                if let Some(text) = chosen {
                    entry.set_text(&text);
                    entry.set_position(-1);
                }
                popover.popdown();
                entry.grab_focus();
            });
        }
        let mut n = ComboNode {
            id: c.id,
            root,
            entry,
            menu,
            list,
            options,
            on_change,
        };
        n.patch(c);
        n
    }

    fn patch(&mut self, c: &Combo) -> bool {
        if c.id != self.id {
            return false;
        }
        put(&self.on_change, &c.on_change);
        set_entry(&self.entry, &c.value, &c.placeholder, c.enabled);
        set_sensitive(&self.menu, c.enabled && !c.options.is_empty());
        if *self.options.borrow() != c.options {
            while let Some(row) = self.list.row_at_index(0) {
                self.list.remove(&row);
            }
            for o in &c.options {
                let l = crate::util::label(o, &["t-branch"]);
                l.set_margin_start(6);
                l.set_margin_end(6);
                l.set_margin_top(3);
                l.set_margin_bottom(3);
                self.list.append(&l);
            }
            *self.options.borrow_mut() = c.options.clone();
        }
        true
    }
}

pub struct SegmentedNode {
    pub id: Id,
    pub root: gtk::Box,
    buttons: Vec<gtk::ToggleButton>,
    options: Vec<String>,
    on_select: Slot<usize>,
}

impl SegmentedNode {
    fn new(s: &Segmented) -> SegmentedNode {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("linked");
        let on_select = slot(&s.on_select);
        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        for (i, o) in s.options.iter().enumerate() {
            let b = gtk::ToggleButton::with_label(o);
            if let Some(first) = buttons.first() {
                b.set_group(Some(first));
            }
            let sl = on_select.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    fire(&sl, i);
                }
            });
            root.append(&b);
            buttons.push(b);
        }
        let mut n = SegmentedNode {
            id: s.id,
            root,
            buttons,
            options: s.options.clone(),
            on_select,
        };
        n.patch(s);
        n
    }

    fn patch(&mut self, s: &Segmented) -> bool {
        if s.id != self.id || s.options != self.options {
            return false;
        }
        put(&self.on_select, &s.on_select);
        if let Some(b) = self.buttons.get(s.selected) {
            if !b.is_active() {
                b.set_active(true);
            }
        }
        true
    }
}

pub struct ChoiceNode {
    pub id: Id,
    pub dropdown: gtk::DropDown,
    options: Vec<String>,
    on_select: Slot<usize>,
}

impl ChoiceNode {
    fn new(c: &Choice) -> ChoiceNode {
        let strs: Vec<&str> = c.options.iter().map(String::as_str).collect();
        let dropdown = gtk::DropDown::from_strings(&strs);
        dropdown.set_halign(gtk::Align::Start);
        let on_select = slot(&c.on_select);
        let s = on_select.clone();
        dropdown.connect_selected_notify(move |d| {
            let i = d.selected();
            if i != gtk::INVALID_LIST_POSITION {
                fire(&s, i as usize);
            }
        });
        let mut n = ChoiceNode {
            id: c.id,
            dropdown,
            options: c.options.clone(),
            on_select,
        };
        n.patch(c);
        n
    }

    fn patch(&mut self, c: &Choice) -> bool {
        if c.id != self.id || c.options != self.options {
            return false;
        }
        put(&self.on_select, &c.on_select);
        if self.dropdown.selected() as usize != c.selected {
            self.dropdown.set_selected(c.selected as u32);
        }
        if let Some(name) = &c.a11y_label {
            self.dropdown
                .update_property(&[gtk::accessible::Property::Label(name)]);
        }
        true
    }
}

// MARK: Forms

struct FormRowNode {
    caption: Option<gtk::Label>,
    content: Node,
}

impl FormRowNode {
    fn new(grid: &gtk::Grid, row: i32, r: &wtm_toolkit::FormRow) -> FormRowNode {
        let caption = (!r.caption.is_empty()).then(|| {
            let l = gtk::Label::new(Some(&r.caption));
            l.set_xalign(1.0);
            l.set_halign(gtk::Align::End);
            l.set_valign(gtk::Align::Center);
            grid.attach(&l, 0, row, 1, 1);
            l
        });
        let content = Node::build(&r.content, Axis::Horizontal);
        if let Some(w) = &content.widget {
            w.set_hexpand(true);
            let wraps = matches!(&r.content, Element::Text(t) if t.wrap);
            if !wraps
                && !matches!(
                    r.content,
                    Element::Field(_) | Element::Combo(_) | Element::Stack(_)
                )
            {
                w.set_halign(gtk::Align::Start);
            }
            grid.attach(w, 1, row, 1, 1);
        }
        let n = FormRowNode { caption, content };
        n.hide(r.hidden);
        n
    }

    fn patch(&mut self, grid: &gtk::Grid, row: i32, r: &wtm_toolkit::FormRow) {
        if let Some(l) = &self.caption {
            if l.text() != r.caption {
                l.set_text(&r.caption);
            }
        }
        if !self.content.patch(&r.content, Axis::Horizontal) {
            if let Some(old) = &self.content.widget {
                grid.remove(old);
            }
            self.content = Node::build(&r.content, Axis::Horizontal);
            if let Some(w) = &self.content.widget {
                w.set_hexpand(true);
                grid.attach(w, 1, row, 1, 1);
            }
        }
        if let Some(w) = &self.content.widget {
            w.set_hexpand(true);
        }
        self.hide(r.hidden);
    }

    fn hide(&self, hidden: bool) {
        if let Some(l) = &self.caption {
            show(l, !hidden);
        }
        if let Some(w) = &self.content.widget {
            show(w, !hidden);
        }
    }
}
