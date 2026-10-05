//! Generic `Element` trees as AppKit views: dialog bodies, the Settings
//! window, the notice bar and the empty state. (The list's rows are not
//! built here; they are the hand-laid cells in `cells.rs`.)
//!
//! A built tree is patched in place on every render while its shape stays
//! the same, so a field being typed in keeps its editor, a popup keeps its
//! menu open, and an alert keeps its size. A tree whose shape changed is
//! built again by the caller.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSBezelStyle, NSButton, NSCellImagePosition, NSColor, NSComboBox,
    NSComboBoxDelegate, NSControl, NSControlSize, NSControlTextEditingDelegate, NSFont,
    NSImageSymbolConfiguration, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultHigh, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired,
    NSLineBreakMode, NSPopUpButton, NSProgressIndicator, NSProgressIndicatorStyle, NSScrollView,
    NSSegmentSwitchTracking, NSSegmentedControl, NSStackView, NSStackViewDistribution,
    NSTextAlignment, NSTextField, NSTextFieldDelegate, NSTextView,
    NSUserInterfaceLayoutOrientation, NSView, NSWindowDelegate,
};
use objc2_foundation::{
    NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
};
use wtm_toolkit::{
    Align, Axis, Button as ButtonEl, ButtonKind, Element, Icon, Id, Ink, Shrink, Stack, Text,
    TextAlign, TextStyle, Tint,
};

use crate::badge::{Badge, BadgeTone};
use crate::branchlabel::rich_label;
use crate::button::{Button, IconButton};
use crate::util::{ns, symbol, MEDIUM, REGULAR, SEMIBOLD};

/// Width of a dialog's form, and of the caption column in it.
pub const FORM_WIDTH: f64 = 420.0;
const LABEL_WIDTH: f64 = 120.0;

// MARK: Callback target

pub struct CallbackIvars {
    f: RefCell<Option<Rc<dyn Fn()>>>,
    /// A combo box's pick from its list, which sends no text-change
    /// notification.
    pick: RefCell<Option<Rc<dyn Fn()>>>,
}

define_class!(
    /// An `NSObject` whose `invoke:` action runs a Rust closure: the target
    /// of a control, the delegate of a text field (run on every keystroke)
    /// or of a combo box (also run on a pick from its list), or of a window
    /// (run when it closes).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMCallback"]
    #[ivars = CallbackIvars]
    pub struct Callback;

    impl Callback {
        #[unsafe(method(invoke:))]
        fn invoke(&self, _sender: Option<&AnyObject>) {
            self.call();
        }
    }

    unsafe impl NSObjectProtocol for Callback {}

    unsafe impl NSControlTextEditingDelegate for Callback {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _n: &NSNotification) {
            self.call();
        }
    }

    unsafe impl NSTextFieldDelegate for Callback {}

    unsafe impl NSComboBoxDelegate for Callback {
        #[unsafe(method(comboBoxSelectionDidChange:))]
        fn combo_selection_did_change(&self, _n: &NSNotification) {
            let pick = self.ivars().pick.borrow().clone();
            if let Some(f) = pick {
                f();
            }
        }
    }

    unsafe impl NSWindowDelegate for Callback {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            self.call();
        }
    }
);

impl Callback {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(CallbackIvars {
            f: RefCell::new(None),
            pick: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn set(&self, f: impl Fn() + 'static) {
        *self.ivars().f.borrow_mut() = Some(Rc::new(f));
    }

    fn set_pick(&self, f: impl Fn() + 'static) {
        *self.ivars().pick.borrow_mut() = Some(Rc::new(f));
    }

    pub fn attach(&self, control: &NSControl) {
        unsafe {
            control.setTarget(Some(self.as_ref()));
            control.setAction(Some(sel!(invoke:)));
        }
    }

    /// Run on every keystroke in `field`. A field holds its delegate weakly,
    /// so the caller keeps this alive as long as the field.
    pub fn watch(&self, field: &NSTextField) {
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
    }

    /// Cloned out first: the closure may lead AppKit to call back in.
    fn call(&self) {
        let f = self.ivars().f.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }
}

// MARK: Fonts, colours, symbols

pub fn font(style: TextStyle) -> Retained<NSFont> {
    match style {
        TextStyle::Body => NSFont::systemFontOfSize(13.0),
        TextStyle::Heading => NSFont::systemFontOfSize_weight(16.0, SEMIBOLD),
        TextStyle::Title => NSFont::systemFontOfSize_weight(14.0, SEMIBOLD),
        TextStyle::Small => NSFont::systemFontOfSize(12.0),
        TextStyle::Caption => NSFont::systemFontOfSize(11.0),
        TextStyle::Path => NSFont::monospacedSystemFontOfSize_weight(10.5, REGULAR),
        TextStyle::Branch => NSFont::monospacedSystemFontOfSize_weight(12.0, REGULAR),
        TextStyle::BranchStrong => NSFont::monospacedSystemFontOfSize_weight(12.0, SEMIBOLD),
    }
}

fn bold(style: TextStyle) -> Retained<NSFont> {
    match style {
        TextStyle::Path | TextStyle::Branch | TextStyle::BranchStrong => {
            NSFont::monospacedSystemFontOfSize_weight(font(style).pointSize(), SEMIBOLD)
        }
        _ => NSFont::systemFontOfSize_weight(font(style).pointSize(), SEMIBOLD),
    }
}

pub fn ink(ink: Ink) -> Retained<NSColor> {
    match ink {
        Ink::Primary => NSColor::labelColor(),
        Ink::Secondary => NSColor::secondaryLabelColor(),
        Ink::Error => NSColor::systemRedColor(),
    }
}

/// The SF Symbol for each of the app's icons.
pub fn symbol_name(icon: Icon) -> &'static str {
    match icon {
        Icon::Add => "plus",
        Icon::Refresh => "arrow.clockwise",
        Icon::Settings => "gearshape",
        Icon::Copy => "doc.on.doc",
        Icon::Push => "arrow.up.to.line",
        Icon::Pull => "arrow.down.to.line",
        Icon::Merge => "arrow.triangle.merge",
        Icon::Editor => "chevron.left.forwardslash.chevron.right",
        Icon::Terminal => "terminal",
        Icon::Folder => "folder",
        Icon::Delete => "trash",
        Icon::Close => "xmark",
        Icon::Chevrons => "chevron.up.chevron.down",
    }
}

/// A borderless SF Symbol button in the row style.
pub fn icon_button(icon: Icon, hint: &str, mtm: MainThreadMarker) -> Retained<IconButton> {
    let image = symbol(symbol_name(icon), hint).unwrap_or_default();
    let b = IconButton::new(&image, hint, mtm);
    b.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    b.setBordered(false);
    b.setImagePosition(NSCellImagePosition::ImageOnly);
    b.setControlSize(NSControlSize::Small);
    b.setSymbolConfiguration(Some(
        &NSImageSymbolConfiguration::configurationWithPointSize_weight(12.0, MEDIUM),
    ));
    b.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    b.setContentHuggingPriority_forOrientation(
        NSLayoutPriorityRequired,
        NSLayoutConstraintOrientation::Horizontal,
    );
    b.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityRequired,
        NSLayoutConstraintOrientation::Horizontal,
    );
    b
}

// MARK: Built trees

/// An element as built, with the native views it is patched through.
pub struct Built {
    pub view: Retained<NSView>,
    node: Node,
}

enum Node {
    Stack(Retained<NSStackView>, Vec<Built>),
    Text(Retained<NSTextField>),
    Button(Retained<NSButton>, Retained<Callback>),
    Badge(Retained<Badge>),
    Spinner(Retained<NSProgressIndicator>),
    Field(Retained<NSTextField>, Retained<Callback>),
    Combo(Retained<NSComboBox>, Retained<Callback>),
    Segmented(Retained<NSSegmentedControl>, Retained<Callback>),
    Choice(Retained<NSPopUpButton>, Retained<Callback>),
    TextBlock(Retained<NSTextView>),
    /// Each row's stack, and what is in it.
    Form(Vec<(Retained<NSStackView>, Built)>),
    /// Spacing, applied by the stack it is in; no view of its own.
    Gap,
    Flex,
}

/// Whether `new` can be patched onto what `old` built.
pub fn same_shape(old: &Element, new: &Element) -> bool {
    match (old, new) {
        (Element::Stack(a), Element::Stack(b)) => {
            a.axis == b.axis
                && a.children.len() == b.children.len()
                && a.children
                    .iter()
                    .zip(&b.children)
                    .all(|(x, y)| same_shape(x, y))
        }
        (Element::Form(a), Element::Form(b)) => {
            a.rows.len() == b.rows.len()
                && a.rows
                    .iter()
                    .zip(&b.rows)
                    .all(|(x, y)| x.caption == y.caption && same_shape(&x.content, &y.content))
        }
        (Element::Button(a), Element::Button(b)) => a.kind == b.kind && a.icon == b.icon,
        (Element::Segmented(a), Element::Segmented(b)) => a.options == b.options,
        (Element::Choice(a), Element::Choice(b)) => a.options == b.options,
        (Element::Text(a), Element::Text(b)) => a.wrap == b.wrap && a.style == b.style,
        (Element::Gap(_), Element::Gap(_)) => true,
        (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

impl Built {
    pub fn new(element: &Element, mtm: MainThreadMarker) -> Built {
        build(element, None, mtm)
    }

    /// Bring the views in line with `new`, which must have the shape of
    /// `old` (see [`same_shape`]). Handlers are taken afresh every time.
    pub fn patch(&self, old: &Element, new: &Element) {
        patch(self, old, new, None);
    }

    /// The view built for the element with id `id`, to put the keyboard in.
    pub fn find(&self, id: Id, element: &Element) -> Option<Retained<NSView>> {
        match (&self.node, element) {
            (Node::Stack(_, kids), Element::Stack(s)) => kids
                .iter()
                .zip(&s.children)
                .find_map(|(b, e)| b.find(id, e)),
            (Node::Form(rows), Element::Form(f)) => rows
                .iter()
                .zip(&f.rows)
                .find_map(|((_, b), r)| b.find(id, &r.content)),
            (Node::Field(v, _), Element::Field(f)) if f.id == id => {
                Some(v.clone().into_super().into_super())
            }
            (Node::Combo(v, _), Element::Combo(c)) if c.id == id => {
                Some(v.clone().into_super().into_super().into_super())
            }
            (Node::Button(v, _), Element::Button(b)) if b.id == id => {
                Some(v.clone().into_super().into_super())
            }
            _ => None,
        }
    }
}

/// The width a wrapping label in a form wraps at.
fn wrap_width(in_form: bool) -> Option<f64> {
    in_form.then_some(FORM_WIDTH - LABEL_WIDTH - 8.0)
}

fn build(element: &Element, form: Option<()>, mtm: MainThreadMarker) -> Built {
    let in_form = form.is_some();
    let (view, node): (Retained<NSView>, Node) = match element {
        Element::Stack(s) => {
            let stack = NSStackView::new(mtm);
            stack.setDistribution(NSStackViewDistribution::Fill);
            let kids: Vec<Built> = s.children.iter().map(|c| build(c, form, mtm)).collect();
            for (k, c) in kids.iter().zip(&s.children) {
                if !matches!(c, Element::Gap(_)) {
                    stack.addArrangedSubview(&k.view);
                }
            }
            style_stack(&stack, s, &kids);
            (stack.clone().into_super(), Node::Stack(stack, kids))
        }
        Element::Text(t) => {
            let field = if t.wrap {
                let f = NSTextField::wrappingLabelWithString(&ns(""), mtm);
                if let Some(w) = wrap_width(in_form) {
                    f.setPreferredMaxLayoutWidth(w);
                }
                f
            } else {
                let f = NSTextField::labelWithString(&ns(""), mtm);
                f.setUsesSingleLineMode(true);
                f.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
                f
            };
            set_text(&field, t);
            (field.clone().into_super().into_super(), Node::Text(field))
        }
        Element::Button(b) => {
            let target = Callback::new(mtm);
            let button: Retained<NSButton> = match b.kind {
                ButtonKind::Icon => icon_button(
                    b.icon.unwrap_or(Icon::Close),
                    b.hint.as_deref().unwrap_or(""),
                    mtm,
                )
                .into_super()
                .into_super(),
                kind => {
                    let button = Button::with_title(&ns(""), None, sel!(invoke:), mtm);
                    match kind {
                        ButtonKind::Small => {
                            button.setControlSize(NSControlSize::Small);
                            button.setBezelStyle(NSBezelStyle::Push);
                            button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
                        }
                        ButtonKind::Accessory => {
                            button.setControlSize(NSControlSize::Small);
                            button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
                            button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
                        }
                        _ => button.setBezelStyle(NSBezelStyle::Push),
                    }
                    if let Some(icon) = b.icon {
                        button.setImage(symbol(symbol_name(icon), "").as_deref());
                        button.setImagePosition(NSCellImagePosition::ImageLeading);
                    }
                    button.into_super()
                }
            };
            target.attach(&button);
            button.setContentHuggingPriority_forOrientation(
                NSLayoutPriorityDefaultHigh,
                NSLayoutConstraintOrientation::Horizontal,
            );
            (
                button.clone().into_super().into_super(),
                Node::Button(button, target),
            )
        }
        Element::Badge(b) => {
            let badge = Badge::new(
                &b.text,
                BadgeTone {
                    hue: b.hue,
                    emphasis: b.emphasis,
                },
                &b.tooltip,
                mtm,
            );
            (badge.clone().into_super(), Node::Badge(badge))
        }
        Element::Spinner(_) => {
            let p = NSProgressIndicator::new(mtm);
            p.setStyle(NSProgressIndicatorStyle::Spinning);
            p.setControlSize(NSControlSize::Small);
            p.setIndeterminate(true);
            p.setDisplayedWhenStopped(false);
            (p.clone().into_super(), Node::Spinner(p))
        }
        Element::Field(f) => {
            let field = NSTextField::textFieldWithString(&ns(&f.value), mtm);
            field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
            let target = Callback::new(mtm);
            target.watch(&field);
            (
                field.clone().into_super().into_super(),
                Node::Field(field, target),
            )
        }
        Element::Combo(c) => {
            let combo = NSComboBox::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::ZERO, NSSize::new(200.0, 24.0)),
            );
            combo.setCompletes(true);
            combo.setNumberOfVisibleItems(12);
            combo.setStringValue(&ns(&c.value));
            let target = Callback::new(mtm);
            unsafe { combo.setDelegate(Some(ProtocolObject::from_ref(&*target))) };
            (
                combo.clone().into_super().into_super().into_super(),
                Node::Combo(combo, target),
            )
        }
        Element::Segmented(s) => {
            let labels: Vec<_> = s.options.iter().map(|o| ns(o)).collect();
            let control = unsafe {
                NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                    &NSArray::from_retained_slice(&labels),
                    NSSegmentSwitchTracking::SelectOne,
                    None,
                    None,
                    mtm,
                )
            };
            let target = Callback::new(mtm);
            target.attach(&control);
            (
                control.clone().into_super().into_super(),
                Node::Segmented(control, target),
            )
        }
        Element::Choice(c) => {
            let popup = NSPopUpButton::initWithFrame_pullsDown(mtm.alloc(), NSRect::ZERO, false);
            for title in &c.options {
                popup.addItemWithTitle(&ns(title));
            }
            if let Some(label) = &c.a11y_label {
                // The caption beside it is a plain label, so the popup would
                // otherwise announce only the option chosen.
                popup.setAccessibilityLabel(Some(&ns(label)));
            }
            // Popups are sized to their widest title; without this a form
            // row's low hugging would stretch it across the whole form.
            popup.setContentHuggingPriority_forOrientation(
                NSLayoutPriorityDefaultHigh,
                NSLayoutConstraintOrientation::Horizontal,
            );
            let target = Callback::new(mtm);
            target.attach(&popup);
            (
                popup.clone().into_super().into_super().into_super(),
                Node::Choice(popup, target),
            )
        }
        Element::TextBlock(_) => {
            // A long message (git's full output) scrolls and can be selected,
            // so it never has to fit the window or the alert.
            let size = NSSize::new(560.0, 320.0);
            let scroll = NSScrollView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, size));
            scroll.setHasVerticalScroller(true);
            scroll.setBorderType(objc2_app_kit::NSBorderType::BezelBorder);
            let tv = NSTextView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, size));
            tv.setEditable(false);
            tv.setSelectable(true);
            tv.setVerticallyResizable(true);
            tv.setHorizontallyResizable(false);
            tv.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
            tv.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
            tv.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
                11.0, REGULAR,
            )));
            scroll.setDocumentView(Some(&tv));
            (scroll.into_super(), Node::TextBlock(tv))
        }
        Element::Form(f) => {
            let stack = NSStackView::new(mtm);
            stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            stack.setAlignment(NSLayoutAttribute::Leading);
            stack.setSpacing(10.0);
            stack.setDistribution(NSStackViewDistribution::Fill);
            let mut rows = Vec::new();
            for r in &f.rows {
                let content = build(&r.content, Some(()), mtm);
                let row = form_row(&r.caption, &content.view, mtm);
                stack.addArrangedSubview(&row);
                rows.push((row, content));
            }
            // The form is sized with every row showing, and an alert keeps
            // that height once it is up. With a row hidden, a Fill stack
            // hands the spare height to its least hugging view, which opened
            // a gap mid-form. This takes it instead, at the bottom.
            let spacer = NSView::new(mtm);
            spacer.setContentHuggingPriority_forOrientation(
                1.0,
                NSLayoutConstraintOrientation::Vertical,
            );
            stack.addArrangedSubview(&spacer);
            if let Some((last, _)) = rows.last() {
                stack.setCustomSpacing_afterView(0.0, last);
            }
            (stack.into_super(), Node::Form(rows))
        }
        Element::Gap(_) => (NSView::new(mtm), Node::Gap),
        Element::Flex => {
            let v = NSView::new(mtm);
            v.setContentHuggingPriority_forOrientation(
                1.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
            v.setContentCompressionResistancePriority_forOrientation(
                1.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
            (v, Node::Flex)
        }
    };
    let built = Built { view, node };
    patch(&built, element, element, Some(()));
    built
}

/// A caption in the left column, `control` in the right.
fn form_row(caption: &str, control: &NSView, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let r = NSStackView::new(mtm);
    r.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    r.setAlignment(NSLayoutAttribute::FirstBaseline);
    r.setSpacing(8.0);
    let l = NSTextField::labelWithString(&ns(caption), mtm);
    l.setAlignment(NSTextAlignment::Right);
    l.widthAnchor()
        .constraintEqualToConstant(LABEL_WIDTH)
        .setActive(true);
    r.addArrangedSubview(&l);
    r.addArrangedSubview(control);
    control.setContentHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultLow,
        NSLayoutConstraintOrientation::Horizontal,
    );
    r.widthAnchor()
        .constraintEqualToConstant(FORM_WIDTH)
        .setActive(true);
    r
}

fn style_stack(stack: &NSStackView, s: &Stack, kids: &[Built]) {
    let horizontal = s.axis == Axis::Horizontal;
    stack.setOrientation(if horizontal {
        NSUserInterfaceLayoutOrientation::Horizontal
    } else {
        NSUserInterfaceLayoutOrientation::Vertical
    });
    stack.setSpacing(s.spacing);
    stack.setAlignment(match (s.align, horizontal) {
        (Align::Start, true) => NSLayoutAttribute::Top,
        (Align::Start, false) => NSLayoutAttribute::Leading,
        (Align::Center, true) => NSLayoutAttribute::CenterY,
        (Align::Center, false) => NSLayoutAttribute::CenterX,
        (Align::Baseline, _) => NSLayoutAttribute::FirstBaseline,
        (Align::Fill, true) => NSLayoutAttribute::Height,
        (Align::Fill, false) => NSLayoutAttribute::Width,
    });
    // A gap replaces the spacing after the view before it.
    let mut previous: Option<&Built> = None;
    for (k, c) in kids.iter().zip(&s.children) {
        match c {
            Element::Gap(g) => {
                if let Some(p) = previous {
                    stack.setCustomSpacing_afterView(*g, &p.view);
                }
            }
            _ => previous = Some(k),
        }
    }
}

/// How hard a view holds its length along a stack: `grow` gives way to take
/// spare room, `Shrink` gives way when there is too little.
fn sizing(view: &NSView, grow: bool, shrink: Shrink) {
    let o = NSLayoutConstraintOrientation::Horizontal;
    if grow {
        view.setContentHuggingPriority_forOrientation(NSLayoutPriorityDefaultLow - 1.0, o);
    }
    match shrink {
        Shrink::Normal => {}
        Shrink::First => view.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow - 10.0,
            o,
        ),
        Shrink::Second => view
            .setContentCompressionResistancePriority_forOrientation(NSLayoutPriorityDefaultLow, o),
    }
}

fn set_text(field: &NSTextField, t: &Text) {
    let face = font(t.style);
    let color = ink(t.ink);
    field.setAttributedStringValue(&rich_label(&t.content, &face, &color, Some(&bold(t.style))));
    field.setFont(Some(&face));
    field.setTextColor(Some(&color));
    field.setAlignment(match t.align {
        TextAlign::Leading => NSTextAlignment::Left,
        TextAlign::Trailing => NSTextAlignment::Right,
        TextAlign::Center => NSTextAlignment::Center,
    });
    field.setToolTip(t.tooltip.as_deref().map(ns).as_deref());
    field.setMaximumNumberOfLines(t.lines as isize);
    if t.lines > 0 {
        field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    }
    field.setSelectable(t.selectable);
    field.setHidden(t.hidden);
    sizing(field, t.grow, t.shrink);
    if t.lines > 0 && t.wrap {
        // A notice must never resize the window: the lines past the limit
        // are cut rather than pushing the bar taller.
        field.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Vertical,
        );
    }
}

fn patch_button(button: &NSButton, target: &Callback, b: &ButtonEl) {
    if b.kind != ButtonKind::Icon {
        let face = button
            .font()
            .unwrap_or_else(|| NSFont::systemFontOfSize(13.0));
        let title = rich_label(&b.label, &face, &NSColor::controlTextColor(), None);
        if button.attributedTitle().string().to_string() != b.label.to_plain() {
            button.setAttributedTitle(&title);
        }
        button.setKeyEquivalent(&ns(if b.is_default { "\r" } else { "" }));
    }
    if let Some(icon_button) = button.downcast_ref::<IconButton>() {
        icon_button.set_hint(b.hint.as_deref().unwrap_or(""));
    } else {
        button.setToolTip(b.hint.as_deref().map(ns).as_deref());
        button.setAccessibilityHelp(b.hint.as_deref().map(ns).as_deref());
    }
    if let Some(label) = &b.a11y_label {
        button.setAccessibilityLabel(Some(&ns(label)));
    }
    button.setEnabled(b.enabled);
    button.setHidden(b.hidden);
    let red = (b.tint == Tint::Danger).then(NSColor::systemRedColor);
    button.setContentTintColor(red.as_deref());
    sizing(button, false, b.shrink);
    let h = b.on_press.clone();
    target.set(move || h.call(()));
}

/// Whether `field` is being typed in: its text is then the user's, and is
/// neither rewritten nor taken away by disabling it.
fn editing(field: &NSTextField) -> bool {
    field.currentEditor().is_some()
}

fn patch(built: &Built, old: &Element, new: &Element, form: Option<()>) {
    let _ = form;
    match (&built.node, new) {
        (Node::Stack(stack, kids), Element::Stack(s)) => {
            let old_children = match old {
                Element::Stack(o) => &o.children,
                _ => &s.children,
            };
            for ((k, o), n) in kids.iter().zip(old_children).zip(&s.children) {
                patch(k, o, n, form);
            }
            style_stack(stack, s, kids);
            stack.setHidden(s.hidden);
            sizing(stack, s.grow, s.shrink);
        }
        (Node::Text(field), Element::Text(t)) => {
            if old != new || field.stringValue().to_string() != t.content.to_plain() {
                set_text(field, t);
            }
        }
        (Node::Button(button, target), Element::Button(b)) => patch_button(button, target, b),
        (Node::Badge(badge), Element::Badge(b)) => badge.set(
            &b.text,
            BadgeTone {
                hue: b.hue,
                emphasis: b.emphasis,
            },
            &b.tooltip,
        ),
        (Node::Spinner(p), Element::Spinner(s)) => {
            if s.spinning {
                unsafe { p.startAnimation(None) };
            } else {
                unsafe { p.stopAnimation(None) };
            }
            p.setToolTip(s.tooltip.as_deref().map(ns).as_deref());
        }
        (Node::Field(field, target), Element::Field(f)) => {
            if field.stringValue().to_string() != f.value {
                field.setStringValue(&ns(&f.value));
            }
            field.setPlaceholderString(Some(&ns(&f.placeholder)));
            if f.enabled || !editing(field) {
                field.setEnabled(f.enabled);
            }
            let (h, me) = (f.on_change.clone(), field.clone());
            target.set(move || h.call(me.stringValue().to_string()));
        }
        (Node::Combo(combo, target), Element::Combo(c)) => {
            let old_options = match old {
                Element::Combo(o) if !std::ptr::eq(old, new) => Some(&o.options),
                _ => None,
            };
            if old_options != Some(&c.options) {
                combo.removeAllItems();
                let items: Vec<_> = c.options.iter().map(|o| ns(o)).collect();
                let items = NSArray::from_retained_slice(&items);
                unsafe {
                    combo.addItemsWithObjectValues(&Retained::cast_unchecked::<NSArray>(items))
                };
            }
            if combo.stringValue().to_string() != c.value {
                combo.setStringValue(&ns(&c.value));
            }
            combo.setPlaceholderString(Some(&ns(&c.placeholder)));
            // Not while it is being typed in: a fetch landing mid-edit
            // would take the keyboard away from it.
            if c.enabled || !editing(combo) {
                combo.setEnabled(c.enabled);
            }
            let (h, me) = (c.on_change.clone(), combo.clone());
            target.set(move || h.call(me.stringValue().to_string()));
            // The pick lands in the field after this notification, so the
            // value is read from the list.
            let (h, me) = (c.on_change.clone(), combo.clone());
            target.set_pick(move || {
                let index = me.indexOfSelectedItem();
                if index >= 0 {
                    if let Some(value) = me
                        .itemObjectValueAtIndex(index)
                        .downcast_ref::<objc2_foundation::NSString>()
                    {
                        h.call(value.to_string());
                    }
                }
            });
        }
        (Node::Segmented(control, target), Element::Segmented(s)) => {
            if control.selectedSegment() != s.selected as isize {
                control.setSelectedSegment(s.selected as isize);
            }
            let (h, me) = (s.on_select.clone(), control.clone());
            target.set(move || h.call(me.selectedSegment().max(0) as usize));
        }
        (Node::Choice(popup, target), Element::Choice(c)) => {
            if popup.indexOfSelectedItem() != c.selected as isize {
                popup.selectItemAtIndex(c.selected as isize);
            }
            let (h, me) = (c.on_select.clone(), popup.clone());
            target.set(move || h.call(me.indexOfSelectedItem().max(0) as usize));
        }
        (Node::TextBlock(tv), Element::TextBlock(text)) => {
            if tv.string().to_string() != *text {
                tv.setString(&ns(text));
            }
        }
        (Node::Form(rows), Element::Form(f)) => {
            let old_rows = match old {
                Element::Form(o) => &o.rows,
                _ => &f.rows,
            };
            for (((row, b), o), n) in rows.iter().zip(old_rows).zip(&f.rows) {
                patch(b, &o.content, &n.content, Some(()));
                row.setHidden(n.hidden);
            }
        }
        (Node::Gap, _) | (Node::Flex, _) => {}
        _ => debug_assert!(false, "patched an element of another shape"),
    }
}
