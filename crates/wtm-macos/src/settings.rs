//! The view's panels as windows of their own: Settings, a standard macOS
//! settings window rather than a sheet. There is no Save or Cancel; every
//! edit is applied as it is made, so the window can simply be closed (⌘W).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBackingStoreType, NSStackView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};
use wtm_toolkit::Panel;

use crate::elements::{same_shape, Built, Callback, FORM_WIDTH};
use crate::util::ns;

const MARGIN: f64 = 20.0;

struct Open {
    key: String,
    window: Retained<NSWindow>,
    panel: Panel,
    body: Built,
    /// The window's delegate, which reports it closing; a window holds its
    /// delegate weakly.
    _delegate: Retained<Callback>,
    /// The view took it away: its closing is not the user's.
    closed_by_view: Rc<Cell<bool>>,
}

thread_local! {
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

/// The window a panel is in, for a folder picker that belongs to it.
pub fn window(key: &str, _mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    OPEN.with(|o| {
        o.borrow()
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.window.clone())
    })
}

/// Bring panel `key` forward.
pub fn present(key: &str, mtm: MainThreadMarker) {
    if let Some(w) = window(key, mtm) {
        w.makeKeyAndOrderFront(None);
    }
}

pub fn render(panels: &[Panel], mtm: MainThreadMarker) {
    // Gone from the view: closed without telling the program, which already
    // knows.
    let gone: Vec<Open> = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let (keep, gone) = o
            .drain(..)
            .partition(|p| panels.iter().any(|n| n.key == p.key));
        *o = keep;
        gone
    });
    for p in gone {
        p.closed_by_view.set(true);
        p.window.close();
    }
    for panel in panels {
        let patched = OPEN.with(|o| {
            let mut o = o.borrow_mut();
            let Some(open) = o.iter_mut().find(|p| p.key == panel.key) else {
                return false;
            };
            if same_shape(&open.panel.body, &panel.body) {
                open.body.patch(&open.panel.body, &panel.body);
            } else {
                open.body = Built::new(&panel.body, mtm);
                fill(&open.window, &open.body);
            }
            if open.panel.title != panel.title {
                open.window.setTitle(&ns(&panel.title));
            }
            open.panel = panel.clone();
            true
        });
        if !patched {
            let open = create(panel, mtm);
            OPEN.with(|o| o.borrow_mut().push(open));
        }
    }
}

fn create(panel: &Panel, mtm: MainThreadMarker) -> Open {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::ZERO, NSSize::new(FORM_WIDTH + 2.0 * MARGIN, 200.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&ns(&panel.title));
    unsafe { window.setReleasedWhenClosed(false) };
    let body = Built::new(&panel.body, mtm);
    fill(&window, &body);
    if let Some(v) = panel.focus.and_then(|id| body.find(id, &panel.body)) {
        window.setInitialFirstResponder(Some(&v));
    }
    window.center();
    // After centring, so a remembered frame wins. Settings keeps the name
    // the window had before panels were generic.
    window.setFrameAutosaveName(&ns(&autosave_name(&panel.key)));

    let closed_by_view = Rc::new(Cell::new(false));
    let delegate = Callback::new(mtm);
    let key = panel.key.clone();
    let flag = closed_by_view.clone();
    delegate.set(move || {
        if flag.get() {
            return;
        }
        // Taken out first: the handler only queues, but the program's next
        // render must not find this window still listed.
        let open = OPEN.with(|o| {
            let mut o = o.borrow_mut();
            let i = o.iter().position(|p| p.key == key)?;
            Some(o.remove(i))
        });
        if let Some(open) = open {
            open.panel.on_close.call(());
            // Not dropped here: this runs inside the window's call to its
            // delegate, which `open` holds the last references to.
            let mtm = MainThreadMarker::new().expect("windows close on the main thread");
            let open = MainThreadBound::new(open, mtm);
            DispatchQueue::main().exec_async(move || drop(open));
        }
    });
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    Open {
        key: panel.key.clone(),
        window,
        panel: panel.clone(),
        body,
        _delegate: delegate,
        closed_by_view,
    }
}

/// `body` as the window's content, inset by the margin, with the window
/// sized to it.
fn fill(window: &NSWindow, body: &Built) {
    let mtm = MainThreadMarker::from(window);
    let holder = NSStackView::new(mtm);
    holder.addArrangedSubview(&body.view);
    holder.setEdgeInsets(NSEdgeInsets {
        top: MARGIN,
        left: MARGIN,
        bottom: MARGIN,
        right: MARGIN,
    });
    holder.layoutSubtreeIfNeeded();
    let size = holder.fittingSize();
    holder.setFrame(NSRect::new(NSPoint::ZERO, size));
    window.setContentSize(size);
    window.setContentView(Some(&holder));
}

fn autosave_name(key: &str) -> String {
    let mut chars = key.chars();
    let title: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    format!("WTM{title}Window")
}
