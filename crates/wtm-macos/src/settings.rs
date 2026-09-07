//! The Settings window: a standard macOS settings window rather than a sheet.
//! There is no Save or Cancel; every edit is applied as it is made, so the
//! window can simply be closed (⌘W) when done. One instance lives for the
//! life of the app and is brought forward on demand.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSControlTextEditingDelegate, NSStackView, NSTextField,
    NSTextFieldDelegate, NSUserInterfaceLayoutOrientation, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
use wtm_core::{Action, App, AppSettings};

use crate::dialogs::{form, hint, pick_folders, row, text_field, Callback, FORM_WIDTH};
use crate::util::ns;

const MARGIN: f64 = 20.0;

pub struct SettingsWindowIvars {
    app: App,
    window: Retained<NSWindow>,
    root: Retained<NSTextField>,
    editor: Retained<NSTextField>,
    /// Target of the Browse… button; kept alive with the window.
    _browse: RefCell<Option<Retained<Callback>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMSettingsWindow"]
    #[ivars = SettingsWindowIvars]
    pub struct SettingsWindow;

    unsafe impl NSObjectProtocol for SettingsWindow {}

    unsafe impl NSWindowDelegate for SettingsWindow {}

    unsafe impl NSControlTextEditingDelegate for SettingsWindow {
        /// Every keystroke applies: the config write is atomic and cheap.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _n: &NSNotification) {
            self.apply();
        }
    }

    unsafe impl NSTextFieldDelegate for SettingsWindow {}
);

impl SettingsWindow {
    pub fn new(app: App, mtm: MainThreadMarker) -> Retained<Self> {
        let config = app.model().config.clone();
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                NSRect::new(NSPoint::ZERO, NSSize::new(FORM_WIDTH + 2.0 * MARGIN, 200.0)),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&ns("Settings"));
        unsafe { window.setReleasedWhenClosed(false) };
        window.setFrameAutosaveName(&ns("WTMSettingsWindow"));

        let root = text_field(&config.worktrees_root, "e.g., ~/.claude-worktrees", mtm);
        let browse = unsafe {
            objc2_app_kit::NSButton::buttonWithTitle_target_action(&ns("Browse…"), None, None, mtm)
        };
        let root_row = NSStackView::new(mtm);
        root_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        root_row.setSpacing(6.0);
        root_row.addArrangedSubview(&root);
        root_row.addArrangedSubview(&browse);
        let editor = text_field(&config.editor_command, "e.g., code", mtm);

        let this = mtm.alloc::<Self>().set_ivars(SettingsWindowIvars {
            app,
            window: window.clone(),
            root: root.clone(),
            editor: editor.clone(),
            _browse: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        unsafe {
            root.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            editor.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        }

        let picker_window = window.clone();
        let root_c = root.clone();
        let me = this.clone();
        let pick = Callback::new(
            move || {
                let root_c = root_c.clone();
                let me = me.clone();
                pick_folders(
                    &picker_window,
                    "Choose worktrees root folder",
                    false,
                    move |paths| {
                        if let Some(p) = paths.first() {
                            root_c.setStringValue(&ns(&p.to_string_lossy()));
                            me.apply();
                        }
                    },
                );
            },
            mtm,
        );
        pick.attach(&browse);
        *this.ivars()._browse.borrow_mut() = Some(pick);

        let f = form(mtm);
        f.addArrangedSubview(&row("Worktrees root:", &root_row, mtm));
        f.addArrangedSubview(&hint(
            "Worktrees are created under this folder, grouped by repo name.",
            mtm,
        ));
        f.addArrangedSubview(&row("Editor command:", &editor, mtm));
        f.addArrangedSubview(&hint(
            "Used by “Open in editor”. The worktree path is appended, or substituted for {path} if present.",
            mtm,
        ));
        f.addArrangedSubview(&hint(
            "“Open in terminal” uses your system default terminal (set via “Set as default terminal” in your terminal app).",
            mtm,
        ));
        f.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: MARGIN,
            left: MARGIN,
            bottom: MARGIN,
            right: MARGIN,
        });
        f.layoutSubtreeIfNeeded();
        let size = f.fittingSize();
        f.setFrame(NSRect::new(NSPoint::ZERO, size));
        window.setContentSize(size);
        window.setContentView(Some(&f));
        window.setInitialFirstResponder(Some(&root));
        window.center();
        this
    }

    /// Bring the window forward, refreshed from the current config.
    pub fn show(&self) {
        let iv = self.ivars();
        let config = iv.app.model().config.clone();
        if !iv.window.isVisible() {
            iv.root.setStringValue(&ns(&config.worktrees_root));
            iv.editor.setStringValue(&ns(&config.editor_command));
        }
        iv.window.makeKeyAndOrderFront(None);
    }

    fn apply(&self) {
        let iv = self.ivars();
        let settings = AppSettings {
            worktrees_root: iv.root.stringValue().to_string().trim().to_string(),
            editor_command: iv.editor.stringValue().to_string().trim().to_string(),
        };
        if settings.worktrees_root.is_empty() {
            // Never persist an empty root; the field is mid-edit.
            return;
        }
        let current = iv.app.model().config.clone();
        if current.worktrees_root != settings.worktrees_root
            || current.editor_command != settings.editor_command
        {
            iv.app.dispatch(Action::SetSettings(settings));
        }
    }
}
