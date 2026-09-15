//! A one-off note shown at the first launch of the Rust app, for people whose
//! Electron copy has just updated into it and wonder why everything looks
//! different. It is an ordinary window, closed the ordinary way; whether it has
//! been shown is remembered in the user defaults, not the config, so importing
//! or editing a config file never brings it back.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSLinkAttributeName, NSStackView, NSTextField, NSTextView, NSUserInterfaceLayoutOrientation,
    NSWindow, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSDictionary, NSEdgeInsets, NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize,
    NSString, NSUserDefaults, NSURL,
};

use crate::util::{ns, SEMIBOLD};

/// Set once the window has been shown. Deleting it from the app's defaults
/// shows the window again at the next launch.
const SEEN: &str = "SEEN_REWRITE_ANNOUNCEMENT";

const HEADING: &str = "What?";
const BEFORE_LINK: &str =
    "Things sure look different! I decided to throw Electron in the trash and \
                           rebuild the whole app as a native app in Rust. The design will probably \
                           change a bit, so feel free to ";
const LINK: &str = "open an issue";
const AFTER_LINK: &str = " if you have any problems.";
const ISSUES: &str = "https://github.com/nino/worktree-manager/issues";

const WIDTH: f64 = 380.0;
const MARGIN: f64 = 24.0;

thread_local! {
    /// The window is not released when closed, so something has to own it.
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

/// Show the announcement unless it has been shown before.
pub fn show_if_unseen(mtm: MainThreadMarker) {
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = ns(SEEN);
    if defaults.boolForKey(&key) {
        return;
    }
    // Marked on opening rather than on closing: quitting with the window still
    // open counts as having seen it.
    defaults.setBool_forKey(true, &key);
    let window = build(mtm);
    window.center();
    window.makeKeyAndOrderFront(None);
    WINDOW.with(|w| *w.borrow_mut() = Some(window));
}

fn build(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH + 2.0 * MARGIN, 200.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    // The heading says it; the title stays for the Window menu and VoiceOver.
    window.setTitle(&ns(HEADING));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitlebarAppearsTransparent(true);

    let heading = NSTextField::labelWithString(&ns(HEADING), mtm);
    heading.setFont(Some(&NSFont::systemFontOfSize_weight(22.0, SEMIBOLD)));

    let body = body_view(mtm);

    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    stack.setSpacing(10.0);
    stack.setEdgeInsets(NSEdgeInsets {
        // The transparent title bar above already reads as margin.
        top: 4.0,
        left: MARGIN,
        bottom: MARGIN,
        right: MARGIN,
    });
    stack.addArrangedSubview(&heading);
    stack.addArrangedSubview(&body);
    stack.layoutSubtreeIfNeeded();
    // A Leading-aligned stack leaves its trailing inset out of the fitting
    // width, so the width is set here and only the height is measured.
    let size = NSSize::new(WIDTH + 2.0 * MARGIN, stack.fittingSize().height);
    stack.setFrame(NSRect::new(NSPoint::ZERO, size));
    window.setContentSize(size);
    window.setContentView(Some(&stack));
    window
}

/// A text view rather than a label: links in a label only become clickable
/// once the label has been clicked into, while a text view opens them on the
/// first click.
fn body_view(mtm: MainThreadMarker) -> Retained<NSTextView> {
    let font = NSFont::systemFontOfSize(13.0);
    let plain = unsafe {
        NSDictionary::from_retained_objects::<NSString>(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[
                Retained::into_super(Retained::into_super(font.clone())),
                Retained::into_super(Retained::into_super(NSColor::labelColor())),
            ],
        )
    };
    let text = unsafe {
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &ns(&format!("{BEFORE_LINK}{LINK}{AFTER_LINK}")),
            Some(&plain),
        )
    };
    let url = NSURL::URLWithString(&ns(ISSUES)).expect("a valid URL");
    let start = BEFORE_LINK.encode_utf16().count();
    unsafe {
        text.addAttribute_value_range(
            NSLinkAttributeName,
            &*Retained::into_super(url) as &AnyObject,
            NSRange::new(start, LINK.encode_utf16().count()),
        )
    };

    let view = NSTextView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, 10.0)),
    );
    view.setEditable(false);
    view.setSelectable(true);
    view.setDrawsBackground(false);
    view.setTextContainerInset(NSSize::ZERO);
    if let Some(container) = unsafe { view.textContainer() } {
        container.setLineFragmentPadding(0.0);
    }
    if let Some(storage) = unsafe { view.textStorage() } {
        storage.setAttributedString(&text);
    }
    // A text view has no intrinsic size, so measure the laid-out text and pin
    // the view to it.
    let height = match (unsafe { view.layoutManager() }, unsafe {
        view.textContainer()
    }) {
        (Some(layout), Some(container)) => {
            layout.ensureLayoutForTextContainer(&container);
            layout
                .usedRectForTextContainer(&container)
                .size
                .height
                .ceil()
        }
        _ => 60.0,
    };
    view.widthAnchor()
        .constraintEqualToConstant(WIDTH)
        .setActive(true);
    view.heightAnchor()
        .constraintEqualToConstant(height)
        .setActive(true);
    view
}
