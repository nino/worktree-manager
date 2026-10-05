//! The view's dialogs as `NSAlert` sheets on the main window, with the body
//! in the alert's accessory view, and folder pickers as `NSOpenPanel` sheets.
//!
//! One dialog is on screen at a time, the first in the view; the shared UI
//! holds the rest back until the turn after one closes, because AppKit
//! queues a sheet begun while another is still animating away.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSButton, NSModalResponse, NSModalResponseOK,
    NSOpenPanel, NSWindow,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use wtm_toolkit::{Dialog, DialogStyle, Element, Role};

use crate::elements::{same_shape, Built, FORM_WIDTH};
use crate::util::ns;

/// The dialog on screen.
struct Open {
    id: u64,
    alert: Retained<NSAlert>,
    /// As last rendered: its buttons' handlers are this render's.
    dialog: Dialog,
    body: Option<Built>,
    buttons: Vec<Retained<NSButton>>,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    /// Dialogs closed here, by one of their buttons: never shown again, even
    /// if a render arrives before the program has taken them out.
    static CLOSED: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

/// Bring the sheet on `window` in line with `dialogs`.
pub fn render(dialogs: &[Dialog], window: &NSWindow, mtm: MainThreadMarker) {
    let first = dialogs
        .iter()
        .find(|d| !CLOSED.with(|c| c.borrow().contains(&d.id)));
    // A sheet's buttons cannot be relabelled or added to once it is up: a
    // dialog whose buttons change is shown again.
    let open = OPEN.with(|o| {
        o.borrow()
            .as_ref()
            .map(|o| (o.id, first.is_some_and(|d| same_buttons(&o.dialog, d))))
    });
    match (open, first) {
        (Some((id, true)), Some(d)) if id == d.id => patch(d),
        (open, first) => {
            if open.is_some() {
                close_by_view(window);
            }
            if let Some(d) = first {
                show(d, window, mtm);
            }
        }
    }
}

fn same_buttons(a: &Dialog, b: &Dialog) -> bool {
    a.buttons.len() == b.buttons.len()
        && a.buttons
            .iter()
            .zip(&b.buttons)
            .all(|(x, y)| x.label == y.label && x.role == y.role)
}

/// The view took the dialog away (its repo was removed, say): close the
/// sheet without running any of its buttons.
fn close_by_view(window: &NSWindow) {
    let open = OPEN.with(|o| o.borrow_mut().take());
    if let Some(open) = open {
        // The completion handler runs inside this and finds nothing open.
        window.endSheet(&open.alert.window());
    }
}

fn style(s: DialogStyle) -> NSAlertStyle {
    match s {
        DialogStyle::Info => NSAlertStyle::Informational,
        DialogStyle::Warning => NSAlertStyle::Warning,
        DialogStyle::Critical => NSAlertStyle::Critical,
    }
}

/// Size the accessory view to its content; NSAlert lays it out by frame. A
/// scrolling text block keeps the size it was built with.
fn size_body(body: &Built, element: &Element) {
    if matches!(element, Element::TextBlock(_)) {
        return;
    }
    body.view.layoutSubtreeIfNeeded();
    let fit = body.view.fittingSize();
    body.view.setFrame(NSRect::new(
        NSPoint::ZERO,
        NSSize::new(fit.width.max(FORM_WIDTH), fit.height),
    ));
}

fn show(d: &Dialog, window: &NSWindow, mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns(&d.title));
    if !d.message.is_empty() {
        alert.setInformativeText(&ns(&d.message));
    }
    alert.setAlertStyle(style(d.style));
    // AppKit makes the first button the default (Return), as the view
    // orders them.
    let buttons: Vec<_> = d
        .buttons
        .iter()
        .map(|b| {
            let button = alert.addButtonWithTitle(&ns(&b.label));
            button.setEnabled(b.enabled);
            match b.role {
                Role::Destructive => button.setHasDestructiveAction(true),
                Role::Cancel => button.setKeyEquivalent(&ns("\u{1b}")),
                Role::Default | Role::Normal => {}
            }
            button
        })
        .collect();
    let body = d.body.as_ref().map(|e| {
        let built = Built::new(e, mtm);
        size_body(&built, e);
        alert.setAccessoryView(Some(&built.view));
        built
    });
    if let (Some(id), Some(body), Some(e)) = (d.focus, &body, &d.body) {
        if let Some(v) = body.find(id, e) {
            alert.window().setInitialFirstResponder(Some(&v));
        }
    }
    let id = d.id;
    let block = RcBlock::new(move |response: NSModalResponse| finished(id, response));
    alert.beginSheetModalForWindow_completionHandler(window, Some(&block));
    OPEN.with(|o| {
        *o.borrow_mut() = Some(Open {
            id,
            alert,
            dialog: d.clone(),
            body,
            buttons,
        })
    });
}

fn patch(d: &Dialog) {
    let mtm = MainThreadMarker::new().expect("rendering happens on the main thread");
    OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let Some(open) = o.as_mut() else { return };
        if open.dialog.title != d.title {
            open.alert.setMessageText(&ns(&d.title));
        }
        if open.dialog.message != d.message {
            open.alert.setInformativeText(&ns(&d.message));
        }
        for (button, b) in open.buttons.iter().zip(&d.buttons) {
            button.setEnabled(b.enabled);
        }
        match (&open.dialog.body, &d.body, &open.body) {
            (Some(old), Some(new), Some(built)) if same_shape(old, new) => built.patch(old, new),
            (_, new, _) => {
                // A body of another shape: built again. The alert keeps the
                // size it opened at, so the view keeps a dialog's shape for
                // as long as it is up; this is the fallback.
                open.body = new.as_ref().map(|e| {
                    let built = Built::new(e, mtm);
                    size_body(&built, e);
                    built
                });
                open.alert
                    .setAccessoryView(open.body.as_ref().map(|b| &*b.view));
            }
        }
        open.dialog = d.clone();
    });
}

/// A button closed the sheet: run its handler, unless the view closed it.
fn finished(id: u64, response: NSModalResponse) {
    let open = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        if o.as_ref().is_some_and(|open| open.id == id) {
            o.take()
        } else {
            None
        }
    });
    let Some(open) = open else { return };
    CLOSED.with(|c| c.borrow_mut().insert(id));
    let index = (response - NSAlertFirstButtonReturn) as usize;
    if let Some(b) = open.dialog.buttons.get(index) {
        b.on_press.call(());
    }
}

// MARK: Folder pickers

/// Open a folder picker as a sheet on `window`; `done` receives the chosen
/// paths (empty when cancelled).
pub fn pick_folders(
    window: &NSWindow,
    title: &str,
    multiple: bool,
    done: impl Fn(Vec<PathBuf>) + 'static,
) {
    let mtm = MainThreadMarker::from(window);
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(multiple);
    panel.setMessage(Some(&ns(title)));
    panel.setPrompt(Some(&ns(if multiple { "Add" } else { "Choose" })));
    let p = panel.clone();
    let block = RcBlock::new(move |resp: NSModalResponse| {
        if resp != NSModalResponseOK {
            done(Vec::new());
            return;
        }
        let paths = p
            .URLs()
            .iter()
            .filter_map(|u| u.path().map(|s| PathBuf::from(s.to_string())))
            .collect();
        done(paths);
    });
    panel.beginSheetModalForWindow_completionHandler(window, &block);
}
