//! Sheets: create worktree, repo settings, delete confirmation, folder
//! pickers (the app settings live in their own window, see `settings.rs`). All are `NSAlert`/`NSOpenPanel` sheets on the main window
//! with native controls in an accessory view; every outcome is dispatched as
//! an [`Action`] and the model update repaints the tree.

use std::cell::RefCell;
use std::path::PathBuf;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSAlertThirdButtonReturn, NSComboBox,
    NSControl, NSFont, NSLayoutAttribute, NSLayoutPriorityDefaultLow, NSModalResponse,
    NSModalResponseOK, NSOpenPanel, NSSegmentSwitchTracking, NSSegmentedControl, NSStackView,
    NSStackViewDistribution, NSTextAlignment, NSTextField, NSUserInterfaceLayoutOrientation,
    NSView, NSWindow,
};
use objc2_foundation::{NSArray, NSObject, NSPoint, NSRect, NSSize, NSString};
use wtm_core::{
    Action, App, CreateWorktreeParams, DeleteRefusal, DeleteWorktreeParams, DeleteWorktreeResult,
};

use crate::util::{label, ns};

pub(crate) const FORM_WIDTH: f64 = 420.0;
const LABEL_WIDTH: f64 = 120.0;

// MARK: Callback target

pub struct CallbackIvars {
    f: RefCell<Option<Box<dyn Fn()>>>,
}

define_class!(
    /// An `NSObject` whose `invoke:` action runs a Rust closure — the target
    /// for controls inside sheets, where there is no long-lived controller.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMCallback"]
    #[ivars = CallbackIvars]
    pub struct Callback;

    impl Callback {
        #[unsafe(method(invoke:))]
        fn invoke(&self, _sender: Option<&AnyObject>) {
            if let Some(f) = self.ivars().f.borrow().as_ref() {
                f();
            }
        }
    }
);

impl Callback {
    pub fn new(f: impl Fn() + 'static, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(CallbackIvars {
            f: RefCell::new(Some(Box::new(f))),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn attach(&self, control: &NSControl) {
        unsafe {
            control.setTarget(Some(self.as_ref()));
            control.setAction(Some(sel!(invoke:)));
        }
    }
}

// MARK: Form helpers

pub(crate) fn form(mtm: MainThreadMarker) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    s.setAlignment(NSLayoutAttribute::Leading);
    s.setSpacing(10.0);
    s.setDistribution(NSStackViewDistribution::Fill);
    s
}

pub(crate) fn row(caption: &str, control: &NSView, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let r = NSStackView::new(mtm);
    r.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    r.setAlignment(NSLayoutAttribute::FirstBaseline);
    r.setSpacing(8.0);
    let l = label(caption, mtm);
    l.setAlignment(NSTextAlignment::Right);
    l.widthAnchor()
        .constraintEqualToConstant(LABEL_WIDTH)
        .setActive(true);
    r.addArrangedSubview(&l);
    r.addArrangedSubview(control);
    control.setContentHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultLow,
        objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
    );
    r.widthAnchor()
        .constraintEqualToConstant(FORM_WIDTH)
        .setActive(true);
    r
}

pub(crate) fn hint(text: &str, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let r = NSStackView::new(mtm);
    r.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    r.setAlignment(NSLayoutAttribute::FirstBaseline);
    r.setSpacing(8.0);
    let pad = NSView::new(mtm);
    pad.widthAnchor()
        .constraintEqualToConstant(LABEL_WIDTH)
        .setActive(true);
    let h = NSTextField::wrappingLabelWithString(&ns(text), mtm);
    h.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    h.setTextColor(Some(&objc2_app_kit::NSColor::secondaryLabelColor()));
    h.setPreferredMaxLayoutWidth(FORM_WIDTH - LABEL_WIDTH - 8.0);
    h.setSelectable(false);
    r.addArrangedSubview(&pad);
    r.addArrangedSubview(&h);
    r.widthAnchor()
        .constraintEqualToConstant(FORM_WIDTH)
        .setActive(true);
    r
}

pub(crate) fn text_field(
    value: &str,
    placeholder: &str,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let f = NSTextField::textFieldWithString(&ns(value), mtm);
    f.setPlaceholderString(Some(&ns(placeholder)));
    f.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    f
}

/// Size the accessory view to its content; NSAlert lays it out by frame.
fn finish(view: &NSStackView) {
    view.layoutSubtreeIfNeeded();
    let h = view.fittingSize().height;
    view.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(FORM_WIDTH, h)));
}

fn alert(title: &str, info: &str, mtm: MainThreadMarker) -> Retained<NSAlert> {
    let a = NSAlert::new(mtm);
    a.setMessageText(&ns(title));
    if !info.is_empty() {
        a.setInformativeText(&ns(info));
    }
    a
}

fn sheet(alert: &NSAlert, window: &NSWindow, on_close: impl Fn(NSModalResponse) + 'static) {
    let block = RcBlock::new(move |resp: NSModalResponse| on_close(resp));
    alert.beginSheetModalForWindow_completionHandler(window, Some(&block));
}

/// A long message (git's full output) in a scrolling, selectable text view,
/// so it never has to fit the window or the alert.
pub fn text_sheet(window: &NSWindow, title: &str, text: &str) {
    let mtm = MainThreadMarker::from(window);
    let a = alert(title, "", mtm);
    a.addButtonWithTitle(&ns("OK"));
    let size = NSSize::new(560.0, 320.0);
    let scroll =
        objc2_app_kit::NSScrollView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, size));
    scroll.setHasVerticalScroller(true);
    scroll.setBorderType(objc2_app_kit::NSBorderType::BezelBorder);
    let tv =
        objc2_app_kit::NSTextView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, size));
    tv.setEditable(false);
    tv.setSelectable(true);
    tv.setVerticallyResizable(true);
    tv.setHorizontallyResizable(false);
    tv.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
    tv.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    tv.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
        11.0,
        crate::util::REGULAR,
    )));
    tv.setString(&ns(text));
    scroll.setDocumentView(Some(&tv));
    a.setAccessoryView(Some(&scroll));
    sheet(&a, window, |_| {});
}

pub fn error_sheet(window: &NSWindow, title: &str, message: &str) {
    let mtm = MainThreadMarker::from(window);
    let a = alert(title, message, mtm);
    a.setAlertStyle(NSAlertStyle::Warning);
    a.addButtonWithTitle(&ns("OK"));
    sheet(&a, window, |_| {});
}

// MARK: Folder pickers

/// Open a folder picker as a sheet; `done` receives the chosen paths (empty
/// when cancelled).
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

pub fn add_repos(app: &App, window: &NSWindow) {
    let app = app.clone();
    pick_folders(window, "Select git repositories", true, move |paths| {
        if !paths.is_empty() {
            app.dispatch(Action::AddRepos(paths));
        }
    });
}

// MARK: Create worktree

pub fn create_worktree(app: &App, window: &NSWindow, repo_id: &str) {
    let mtm = MainThreadMarker::from(window);
    let model = app.model();
    let Some(node) = model.repo(repo_id) else {
        return;
    };
    let a = alert(&format!("New worktree — {}", node.repo.name), "", mtm);
    a.addButtonWithTitle(&ns("Create"));
    a.addButtonWithTitle(&ns("Cancel"));

    let branch = text_field("", "e.g., feature/my-thing", mtm);
    let modes = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&[ns("New branch"), ns("Existing branch")]),
            NSSegmentSwitchTracking::SelectOne,
            None,
            None,
            mtm,
        )
    };
    modes.setSelectedSegment(0);
    let base = NSComboBox::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(200.0, 24.0)),
    );
    base.setCompletes(true);
    base.setNumberOfVisibleItems(12);
    let candidates: Vec<Retained<NSString>> =
        node.base_ref_candidates.iter().map(|s| ns(s)).collect();
    let candidates = NSArray::from_retained_slice(&candidates);
    unsafe { base.addItemsWithObjectValues(&Retained::cast_unchecked::<NSArray>(candidates)) };
    base.setStringValue(&ns(&node.default_base_ref));
    base.setPlaceholderString(Some(&ns(&format!("e.g., {}", node.default_base_ref))));

    let base_row = row("Base ref:", &base, mtm);
    let modes_c = modes.clone();
    let base_row_c = base_row.clone();
    let toggle = Callback::new(
        move || base_row_c.setHidden(modes_c.selectedSegment() != 0),
        mtm,
    );
    toggle.attach(&modes);

    let f = form(mtm);
    f.addArrangedSubview(&row("Branch name:", &branch, mtm));
    f.addArrangedSubview(&row("", &modes, mtm));
    f.addArrangedSubview(&base_row);
    finish(&f);
    a.setAccessoryView(Some(&f));
    a.window().setInitialFirstResponder(Some(&branch));

    let app = app.clone();
    let repo_id = repo_id.to_string();
    let default_base = node.default_base_ref.clone();
    let _keep = toggle; // lives as long as the block below
    sheet(&a, window, move |resp| {
        let _ = &_keep;
        if resp != NSAlertFirstButtonReturn {
            return;
        }
        let name = branch.stringValue().to_string();
        if name.trim().is_empty() {
            return;
        }
        let new_branch = modes.selectedSegment() == 0;
        let base_ref = if new_branch {
            let b = base.stringValue().to_string();
            Some(if b.trim().is_empty() {
                default_base.clone()
            } else {
                b.trim().to_string()
            })
        } else {
            None
        };
        app.dispatch(Action::CreateWorktree(CreateWorktreeParams {
            repo_id: repo_id.clone(),
            branch: name.trim().to_string(),
            new_branch,
            base_ref,
        }));
    });
}

// MARK: Repo settings

pub fn repo_settings(app: &App, window: &NSWindow, repo_id: &str) {
    let mtm = MainThreadMarker::from(window);
    let model = app.model();
    let Some(node) = model.repo(repo_id) else {
        return;
    };
    let repo = node.repo.clone();
    let a = alert(
        &format!("Repo settings — {}", repo.name),
        &app.display_path(&repo.path),
        mtm,
    );
    a.addButtonWithTitle(&ns("Save"));
    a.addButtonWithTitle(&ns("Cancel"));
    a.addButtonWithTitle(&ns("Remove Repo…"));

    let name = text_field(&repo.name, "e.g., my-app", mtm);
    let main = text_field(&repo.main_branch, "e.g., main", mtm);
    let init = text_field(&repo.init_command, "e.g., pnpm i", mtm);
    let f = form(mtm);
    f.addArrangedSubview(&row("Display name:", &name, mtm));
    f.addArrangedSubview(&row("Main branch:", &main, mtm));
    f.addArrangedSubview(&hint(
        "Worktrees show ahead/behind counts relative to this branch.",
        mtm,
    ));
    f.addArrangedSubview(&row("Init command:", &init, mtm));
    f.addArrangedSubview(&hint(
        "Runs inside each new worktree after it is created.",
        mtm,
    ));
    finish(&f);
    a.setAccessoryView(Some(&f));
    a.window().setInitialFirstResponder(Some(&name));

    let app = app.clone();
    sheet(&a, window, move |resp| {
        if resp == NSAlertFirstButtonReturn {
            let mut updated = repo.clone();
            let n = name.stringValue().to_string();
            updated.name = if n.trim().is_empty() {
                repo.name.clone()
            } else {
                n.trim().to_string()
            };
            let m = main.stringValue().to_string();
            updated.main_branch = if m.trim().is_empty() {
                "main".into()
            } else {
                m.trim().to_string()
            };
            updated.init_command = init.stringValue().to_string();
            app.dispatch(Action::UpdateRepo(updated));
        } else if resp == NSAlertThirdButtonReturn {
            let app = app.clone();
            let repo = repo.clone();
            // The first sheet must finish dismissing before another can open.
            dispatch2::DispatchQueue::main().exec_async(move || {
                let mtm = MainThreadMarker::new().expect("main queue");
                let Some(window) = crate::controller::main_window(mtm) else {
                    return;
                };
                let c = alert(
                    &format!("Remove “{}” from the list?", repo.name),
                    "This does not touch any files on disk.",
                    mtm,
                );
                c.addButtonWithTitle(&ns("Remove"))
                    .setHasDestructiveAction(true);
                c.addButtonWithTitle(&ns("Cancel"));
                let id = repo.id.clone();
                sheet(&c, &window, move |r| {
                    if r == NSAlertFirstButtonReturn {
                        app.dispatch(Action::RemoveRepo(id.clone()));
                    }
                });
            });
        }
    });
}

// MARK: Delete worktree

/// First rung of the ladder: confirm, then ask the core. A "dirty" refusal
/// comes back through [`delete_finished`] and opens the force confirmation.
pub fn confirm_delete(app: &App, window: &NSWindow, repo_id: &str, path: &str) {
    let mtm = MainThreadMarker::from(window);
    let model = app.model();
    let Some(w) = model.repo(repo_id).and_then(|n| n.worktree(path)) else {
        return;
    };
    let branch = w.branch.clone();
    let shown = branch.clone().unwrap_or_else(|| "(detached)".into());
    let mut info = String::new();
    if w.prunable {
        info.push_str("The folder is already gone — this cleans up git's bookkeeping. ");
    }
    match &w.status {
        Some(s) if s.is_dirty() => info.push_str("It has uncommitted changes. "),
        None if !w.prunable => info.push_str("Its status could not be determined. "),
        _ => {}
    }
    info.push_str(&app.display_path(path));
    let a = alert(&format!("Delete worktree “{shown}”?"), &info, mtm);
    a.addButtonWithTitle(&ns("Delete"))
        .setHasDestructiveAction(true);
    a.addButtonWithTitle(&ns("Cancel"));
    let app = app.clone();
    let repo_id = repo_id.to_string();
    let path = path.to_string();
    sheet(&a, window, move |resp| {
        if resp == NSAlertFirstButtonReturn {
            request_delete(&app, repo_id.clone(), path.clone(), branch.clone(), false);
        }
    });
}

fn request_delete(app: &App, repo_id: String, path: String, branch: Option<String>, force: bool) {
    let params = DeleteWorktreeParams {
        repo_id: repo_id.clone(),
        worktree_path: path.clone(),
        expected_branch: branch.clone(),
        force,
    };
    let app2 = app.clone();
    app.dispatch(Action::DeleteWorktree(
        params,
        Box::new(move |result| {
            let app = app2.clone();
            dispatch2::DispatchQueue::main()
                .exec_async(move || delete_finished(&app, repo_id, path, branch, result));
        }),
    ));
}

fn delete_finished(
    app: &App,
    repo_id: String,
    path: String,
    branch: Option<String>,
    result: DeleteWorktreeResult,
) {
    let mtm = MainThreadMarker::new().expect("main queue");
    let Some(window) = crate::controller::main_window(mtm) else {
        return;
    };
    if result.ok {
        return;
    }
    match result.reason {
        Some(DeleteRefusal::Dirty) => {
            let shown = branch.clone().unwrap_or_else(|| "(detached)".into());
            let a = alert(
                &format!("“{shown}” has uncommitted changes"),
                "They will be permanently lost. Delete anyway?",
                mtm,
            );
            a.setAlertStyle(NSAlertStyle::Critical);
            a.addButtonWithTitle(&ns("Force Delete — Discard Changes"))
                .setHasDestructiveAction(true);
            a.addButtonWithTitle(&ns("Cancel"));
            let app = app.clone();
            sheet(&a, &window, move |resp| {
                if resp == NSAlertFirstButtonReturn {
                    request_delete(&app, repo_id.clone(), path.clone(), branch.clone(), true);
                }
            });
        }
        _ => {
            // The row was stale (branch changed, worktree gone…): show why and
            // bring the tree back in line with git.
            error_sheet(&window, "Could not delete worktree", &result.message);
            app.dispatch(Action::RefreshRepo(repo_id));
        }
    }
}
