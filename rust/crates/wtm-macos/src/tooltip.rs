//! Tooltips for the row icons. The system's appear after about a second and a
//! half, which is too long for a row of unlabelled glyphs, and AppKit gives no
//! way to shorten that — so these are ours: a small panel shown from the
//! buttons' tracking areas after [`DELAY`], and immediately while another one
//! is already up, the way a menu bar hands over between menus.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSFont, NSPanel, NSScreen, NSTextField, NSView,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::util::ns;

/// How long the pointer must rest on an icon before its tooltip appears.
const DELAY: Duration = Duration::from_millis(250);
/// Space between the icon and the tooltip.
const OFFSET: f64 = 5.0;
const PAD_X: f64 = 7.0;
const PAD_Y: f64 = 4.0;
const RADIUS: f64 = 5.0;

thread_local! {
    static TIP: RefCell<Option<Retained<Tooltip>>> = const { RefCell::new(None) };
    /// Bumped by every show or hide, so a delayed show that has been
    /// overtaken does nothing.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

pub struct TooltipIvars {
    label: Retained<NSTextField>,
}

define_class!(
    /// The panel itself: a rounded plate with one line of text, above
    /// everything and deaf to the mouse.
    #[unsafe(super(NSPanel, NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMTooltip"]
    #[ivars = TooltipIvars]
    pub struct Tooltip;
);

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMTooltipBack"]
    pub struct TooltipBack;

    impl TooltipBack {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let b = self.bounds();
            let rect = NSRect::new(
                NSPoint::new(0.5, 0.5),
                NSSize::new(b.size.width - 1.0, b.size.height - 1.0),
            );
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, RADIUS, RADIUS);
            NSColor::windowBackgroundColor().setFill();
            path.fill();
            NSColor::separatorColor()
                .colorWithAlphaComponent(0.6)
                .setStroke();
            path.setLineWidth(1.0);
            path.stroke();
        }
    }
);

impl Tooltip {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let label = NSTextField::labelWithString(&ns(""), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        label.setTextColor(Some(&NSColor::labelColor()));
        let this = mtm.alloc::<Self>().set_ivars(TooltipIvars {
            label: label.clone(),
        });
        let this: Retained<Self> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: NSRect::new(NSPoint::ZERO, NSSize::new(10.0, 10.0)),
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: true,
            ]
        };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setHasShadow(true);
        this.setIgnoresMouseEvents(true);
        this.setLevel(objc2_app_kit::NSPopUpMenuWindowLevel as isize);
        this.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        unsafe { this.setReleasedWhenClosed(false) };
        let back: Retained<TooltipBack> = unsafe {
            msg_send![mtm.alloc::<TooltipBack>(), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(10.0, 10.0))]
        };
        back.addSubview(&label);
        this.setContentView(Some(&back));
        this
    }

    /// Size to `text` and place the panel under `view`, kept on screen.
    fn present(&self, text: &str, view: &NSView) {
        let iv = self.ivars();
        iv.label.setStringValue(&ns(text));
        iv.label.sizeToFit();
        let size = iv.label.frame().size;
        let panel = NSSize::new(size.width + 2.0 * PAD_X, size.height + 2.0 * PAD_Y);
        iv.label
            .setFrameOrigin(NSPoint::new(PAD_X, (panel.height - size.height) / 2.0));

        let Some(window) = view.window() else { return };
        let in_window = view.convertRect_toView(view.bounds(), None);
        let on_screen = window.convertRectToScreen(in_window);
        let mut x = on_screen.origin.x + (on_screen.size.width - panel.width) / 2.0;
        let mut y = on_screen.origin.y - panel.height - OFFSET;
        if let Some(screen) = window
            .screen()
            .or_else(|| NSScreen::mainScreen(MainThreadMarker::from(self)))
        {
            let frame = screen.visibleFrame();
            x = x.clamp(
                frame.origin.x + 4.0,
                frame.origin.x + frame.size.width - panel.width - 4.0,
            );
            if y < frame.origin.y + 4.0 {
                // No room below: sit above the icon instead.
                y = on_screen.origin.y + on_screen.size.height + OFFSET;
            }
        }
        self.setFrame_display(NSRect::new(NSPoint::new(x, y), panel), true);
        if let Some(back) = self.contentView() {
            back.setNeedsDisplay(true);
        }
        unsafe { window.addChildWindow_ordered(self, objc2_app_kit::NSWindowOrderingMode::Above) };
        self.orderFront(None);
    }
}

/// Show `text` for `view` once the pointer has rested on it, or at once when
/// a tooltip is already up.
pub fn schedule(view: &NSView, text: &str) {
    let mtm = MainThreadMarker::from(view);
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    if visible() {
        present(view, text, mtm);
        return;
    }
    let view = MainThreadBound::new(view.retain(), mtm);
    let text = text.to_string();
    let _ =
        DispatchQueue::main().after(DispatchTime::NOW.time(DELAY.as_nanos() as i64), move || {
            let mtm = MainThreadMarker::new().expect("main queue");
            if GENERATION.with(|g| g.get()) != generation {
                return;
            }
            present(view.get(mtm), &text, mtm);
        });
}

/// Take the tooltip away, and cancel any that is waiting to appear.
pub fn hide() {
    GENERATION.with(|g| g.set(g.get() + 1));
    if let Some(tip) = TIP.with(|t| t.borrow().clone()) {
        if let Some(parent) = tip.parentWindow() {
            parent.removeChildWindow(&tip);
        }
        tip.orderOut(None);
    }
}

fn visible() -> bool {
    TIP.with(|t| t.borrow().as_ref().is_some_and(|tip| tip.isVisible()))
}

fn present(view: &NSView, text: &str, mtm: MainThreadMarker) {
    if text.is_empty() || view.window().is_none() {
        return;
    }
    let tip = TIP.with(|t| {
        t.borrow_mut()
            .get_or_insert_with(|| Tooltip::new(mtm))
            .clone()
    });
    tip.present(text, view);
}
