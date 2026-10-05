//! The backend contract, checked against real GTK widgets: rows patched in
//! place, focus and fields left alone, and no change the backend made itself
//! reported back. Needs a display; skipped without one:
//!
//! ```sh
//! xvfb-run -a cargo test -p wtm-gtk --test conformance
//! ```
//!
//! GTK must be driven from the thread that initialised it, so this runs its
//! own `main` (`harness = false`) and the cases one after another.

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
fn main() {
    linux::main()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::rc::Rc;

    use wtm_gtk::gtk;
    use wtm_gtk::gtk::prelude::*;
    use wtm_gtk::testing::{self, Probe};
    use wtm_toolkit::{
        Badge, Dialog, DialogButton, DialogStyle, Effect, Element, Emphasis, Field, FilterList,
        Form, Handler, Hue, Icon, ListItem, Menu, MenuItem, MenuRole, Popover, RepoHeader, Rich,
        Role, Row, RowAction, RowContent, Section, Standard, Tint, ToolItem, TreeList, View,
        WorktreeRow, BRANCH_BUTTON,
    };

    type Calls<A> = Rc<RefCell<Vec<A>>>;

    /// A handler that records what it was called with.
    fn rec<A: 'static>() -> (Handler<A>, Calls<A>) {
        let calls: Calls<A> = Rc::default();
        let c = calls.clone();
        (Handler::new(move |a| c.borrow_mut().push(a)), calls)
    }

    fn badge(text: &str, hue: Hue) -> Badge {
        Badge {
            text: text.into(),
            hue,
            emphasis: Emphasis::Quiet,
            tooltip: text.into(),
        }
    }

    fn header(name: &str) -> RepoHeader {
        RepoHeader {
            name: name.into(),
            meta: "main · 2 worktrees".into(),
            path: format!("~/code/{name}"),
            path_full: format!("/home/u/code/{name}"),
            error: None,
            loading: false,
            can_create: true,
            on_new_worktree: Handler::none(),
            on_settings: Handler::none(),
            on_copy_path: Handler::none(),
        }
    }

    fn worktree(branch: &str, badges: Vec<Badge>, push: Handler) -> WorktreeRow {
        let action = |id, icon, on_press| RowAction {
            id,
            icon,
            hint: id.to_string(),
            enabled: true,
            hidden: false,
            tint: Tint::Normal,
            on_press,
        };
        WorktreeRow {
            branch: Rich::plain(branch),
            branch_name: branch.into(),
            can_switch: true,
            switch_hint: format!("Switch branch (current: {branch})"),
            on_switch: Handler::none(),
            on_copy_branch: Some(Handler::none()),
            badges,
            busy: None,
            path: format!("~/wt/{branch}"),
            path_full: format!("/home/u/wt/{branch}"),
            on_copy_path: Handler::none(),
            actions: vec![
                vec![action("push", Icon::Push, push)],
                vec![action("editor", Icon::Editor, Handler::none())],
            ],
        }
    }

    /// One card with two worktrees, `w:/a` and `w:/b`.
    fn view(badges_a: Vec<Badge>, push_a: Handler) -> View {
        let mut v = View::default();
        v.window.title = "Worktree Manager".into();
        v.window.min_size = (640.0, 400.0);
        v.window.list = TreeList {
            sections: vec![Section {
                key: "r:1".into(),
                header: header("app"),
                expanded: true,
                rows: vec![
                    Row {
                        key: "w:/a".into(),
                        content: RowContent::Worktree(worktree("main", badges_a, push_a)),
                    },
                    Row {
                        key: "w:/b".into(),
                        content: RowContent::Worktree(worktree(
                            "feature",
                            vec![badge("✓", Hue::Green)],
                            Handler::none(),
                        )),
                    },
                ],
            }],
            ..TreeList::default()
        };
        v
    }

    fn plain() -> View {
        view(vec![badge("✓", Hue::Green)], Handler::none())
    }

    // MARK: Cases

    fn equal_rows_keep_their_widgets() {
        let p = Probe::default();
        let (push, pushed) = rec::<()>();
        p.render(&view(vec![badge("✓", Hue::Green)], Handler::none()));
        let row = p.row("w:/a").expect("row");
        let button = p.element("w:/a", "push").expect("push button");
        let header = p.row("r:1").expect("header");
        // The same row again, then one whose only change is its handler.
        p.render(&view(vec![badge("✓", Hue::Green)], Handler::none()));
        p.render(&view(vec![badge("✓", Hue::Green)], push));
        assert_eq!(p.row("w:/a").unwrap(), row, "row widget replaced");
        assert_eq!(
            p.element("w:/a", "push").unwrap(),
            button,
            "button replaced"
        );
        assert_eq!(p.row("r:1").unwrap(), header, "header replaced");
        // An equal row still takes its new handlers.
        button.downcast_ref::<gtk::Button>().unwrap().emit_clicked();
        testing::pump();
        assert_eq!(pushed.borrow().len(), 1, "the new handler was not taken");
        // A status change patches the same row.
        p.render(&view(
            vec![badge("staged", Hue::Blue), badge("↑2 main", Hue::Teal)],
            Handler::none(),
        ));
        assert_eq!(
            p.row("w:/a").unwrap(),
            row,
            "row replaced on a status change"
        );
        assert_eq!(p.badges("w:/a"), ["staged", "↑2 main"]);
    }

    fn a_badge_change_keeps_focus_on_a_row_button() {
        let p = Probe::default();
        p.render(&plain());
        let button = p.element("w:/a", "push").expect("push button");
        assert!(button.grab_focus(), "push button would not take focus");
        testing::pump();
        assert_eq!(p.focus().as_ref(), Some(&button));
        p.render(&view(vec![badge("unstaged", Hue::Orange)], Handler::none()));
        p.render(&view(
            vec![
                badge("unstaged", Hue::Orange),
                badge("untracked", Hue::Purple),
            ],
            Handler::none(),
        ));
        assert_eq!(
            p.focus().as_ref(),
            Some(&button),
            "focus moved off the button"
        );
        assert_eq!(p.badges("w:/a"), ["unstaged", "untracked"]);
    }

    fn dialog_view(id: u64, value: &str, enabled: bool, on_change: Handler<String>) -> View {
        let mut v = plain();
        v.dialogs = vec![Dialog {
            id,
            title: "New worktree — app".into(),
            message: String::new(),
            style: DialogStyle::Info,
            body: Some(Element::Form(Form::default().row(
                "Branch name:",
                Field {
                    id: "branch",
                    value: value.into(),
                    placeholder: "e.g., feature/my-thing".into(),
                    enabled,
                    on_change,
                },
            ))),
            buttons: vec![
                DialogButton {
                    label: "Create".into(),
                    role: Role::Default,
                    enabled: true,
                    on_press: Handler::none(),
                },
                DialogButton {
                    label: "Cancel".into(),
                    role: Role::Cancel,
                    enabled: true,
                    on_press: Handler::none(),
                },
            ],
            focus: Some("branch"),
        }];
        v
    }

    fn a_focused_field_is_not_rewritten() {
        let p = Probe::default();
        let (on_change, changes) = rec::<String>();
        p.render(&dialog_view(7, "", true, on_change.clone()));
        let entry = p
            .dialog_element("branch")
            .and_then(|w| w.downcast::<gtk::Entry>().ok())
            .expect("branch field");
        assert!(changes.borrow().is_empty(), "opening reported a change");
        // The field has the keyboard (the dialog's `focus`), and the user
        // types: reported once.
        assert!(
            testing_has_focus(&p, &entry),
            "the field did not get the keyboard"
        );
        entry.set_text("abc");
        entry.set_position(-1);
        testing::pump();
        assert_eq!(*changes.borrow(), ["abc"]);
        // A view a keystroke behind must not take the typing back, and the
        // same value must not be written again.
        p.render(&dialog_view(7, "ab", true, on_change.clone()));
        assert_eq!(entry.text(), "abc", "a focused field was rewritten");
        p.render(&dialog_view(7, "abc", false, on_change.clone()));
        assert_eq!(entry.text(), "abc");
        assert_eq!(
            changes.borrow().len(),
            1,
            "the backend reported its own write"
        );
        // Disabled while it has the keyboard: left enabled.
        assert!(
            entry.is_sensitive(),
            "a field with the keyboard was disabled"
        );
        assert_eq!(entry.position(), 3, "the cursor moved");
    }

    fn testing_has_focus(p: &Probe, w: &impl IsA<gtk::Widget>) -> bool {
        let w = w.upcast_ref::<gtk::Widget>();
        p.focus().is_some_and(|f| &f == w || f.is_ancestor(w))
    }

    fn selection_from_the_view_is_not_reported() {
        let p = Probe::default();
        let (on_select, selects) = rec::<Option<String>>();
        let (on_toggle, toggles) = rec::<(String, bool)>();
        let mut v = plain();
        v.window.list.on_select = on_select.clone();
        v.window.list.on_toggle = on_toggle.clone();
        v.window.list.selected = Some("w:/a".into());
        p.render(&v);
        assert!(p.is_selected("w:/a"));
        // The list takes the keyboard on the row already selected.
        p.perform(Effect::FocusList);
        v.window.list.selected = Some("w:/b".into());
        v.window.list.sections[0].expanded = false;
        p.render(&v);
        v.window.list.sections[0].expanded = true;
        p.render(&v);
        assert!(p.is_selected("w:/b") && !p.is_selected("w:/a"));
        assert!(
            selects.borrow().is_empty(),
            "reported: {:?}",
            selects.borrow()
        );
        assert!(
            toggles.borrow().is_empty(),
            "reported: {:?}",
            toggles.borrow()
        );
        // The user's own move is reported.
        p.row("r:1").unwrap().grab_focus();
        testing::pump();
        assert_eq!(*selects.borrow(), [Some("r:1".to_string())]);
        assert!(p.is_selected("r:1") && !p.is_selected("w:/b"));
    }

    fn dialog_buttons_close_then_run_and_never_come_back() {
        let p = Probe::default();
        let (create, created) = rec::<()>();
        let mut v = dialog_view(9, "x", true, Handler::none());
        v.dialogs[0].buttons[0].on_press = create.clone();
        p.render(&v);
        let dialog = p.dialog().expect("dialog shown");
        // What the handler sees when it runs: the dialog already gone.
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut v2 = v.clone();
        let (s, c, d) = (seen.clone(), create.clone(), dialog.clone());
        v2.dialogs[0].buttons[0].on_press = Handler::new(move |()| {
            s.borrow_mut().push(d.is_visible());
            c.call(());
        });
        p.render(&v2);
        // The first button is the default (what Return presses).
        dialog
            .default_widget()
            .and_downcast::<gtk::Button>()
            .expect("a default button")
            .emit_clicked();
        testing::pump();
        assert_eq!(created.borrow().len(), 1);
        assert_eq!(*seen.borrow(), [false], "the handler ran before the close");
        // Still listed (its message is not handled yet): not shown again.
        p.render(&v2);
        assert!(p.dialog().is_none(), "a closed dialog came back");
        // A dialog the view drops is closed without running anything.
        let (cancel, cancelled) = rec::<()>();
        let mut v3 = dialog_view(10, "", true, Handler::none());
        v3.dialogs[0].buttons[1].on_press = cancel;
        p.render(&v3);
        assert!(p.dialog().is_some());
        p.render(&plain());
        assert!(p.dialog().is_none());
        assert!(cancelled.borrow().is_empty());
        assert!(created.borrow().len() == 1);
    }

    fn picker(id: u64, on_dismiss: Handler, on_query: Handler<String>) -> Popover {
        Popover {
            id,
            anchor: ("w:/a".into(), BRANCH_BUTTON),
            list: FilterList {
                query: String::new(),
                placeholder: "e.g., main".into(),
                items: vec![
                    ListItem {
                        label: Rich::plain("main"),
                        checked: true,
                    },
                    ListItem {
                        label: Rich::plain("feature"),
                        checked: false,
                    },
                ],
                selected: Some(0),
                on_query,
                on_move: Handler::none(),
                on_choose: Handler::none(),
                on_dismiss,
            },
        }
    }

    fn only_the_users_dismissal_of_the_popover_is_reported() {
        let p = Probe::default();
        let (dismiss, dismissed) = rec::<()>();
        let (query, queries) = rec::<String>();
        let mut v = plain();
        v.popover = Some(picker(1, dismiss.clone(), query.clone()));
        p.render(&v);
        assert!(p.picker_entry().is_some(), "popover not shown");
        // The view drops it: closed without a word.
        p.render(&plain());
        assert!(p.picker_entry().is_none());
        assert!(dismissed.borrow().is_empty());
        assert!(queries.borrow().is_empty());
        // A new one, which the user closes.
        v.popover = Some(picker(2, dismiss.clone(), query.clone()));
        p.render(&v);
        let entry = p.picker_entry().expect("second popover");
        entry.set_text("fe");
        testing::pump();
        assert_eq!(*queries.borrow(), ["fe"]);
        let popover = entry
            .ancestor(gtk::Popover::static_type())
            .and_downcast::<gtk::Popover>()
            .unwrap();
        popover.popdown();
        testing::pump();
        assert_eq!(dismissed.borrow().len(), 1);
    }

    /// The first descendant of `root` with the CSS class `class`.
    fn find_class(root: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
        if root.has_css_class(class) {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(c) = child {
            if let Some(hit) = find_class(&c, class) {
                return Some(hit);
            }
            child = c.next_sibling();
        }
        None
    }

    fn alert(id: u64, style: DialogStyle, buttons: Vec<DialogButton>) -> View {
        let mut v = plain();
        v.dialogs = vec![Dialog {
            id,
            title: "Something happened".into(),
            message: String::new(),
            style,
            body: None,
            buttons,
            focus: None,
        }];
        v
    }

    fn button(label: &str, role: Role, on_press: Handler) -> DialogButton {
        DialogButton {
            label: label.into(),
            role,
            enabled: true,
            on_press,
        }
    }

    fn an_info_dialog_opens_without_icon_message_or_body() {
        let p = Probe::default();
        p.render(&alert(
            20,
            DialogStyle::Info,
            vec![button("OK", Role::Default, Handler::none())],
        ));
        let d = p.dialog().expect("dialog shown");
        let root: gtk::Widget = d.clone().upcast();
        let icon = find_class(&root, "wtm-dialog-icon").expect("icon");
        // Hidden before the window was first shown, not on a later render.
        assert!(!icon.get_visible(), "an Info dialog shows an icon column");
        let title = find_class(&root, "wtm-dialog-title").expect("title");
        let text = title.parent().unwrap();
        let mut child = text.first_child();
        let mut shown = Vec::new();
        while let Some(c) = child {
            if c.get_visible() {
                shown.push(c.clone());
            }
            child = c.next_sibling();
        }
        assert_eq!(shown, [title], "an empty message or body is shown");
    }

    fn closing_a_dialog_is_escape() {
        let p = Probe::default();
        // One button: closing presses it.
        let (ok, oked) = rec::<()>();
        p.render(&alert(
            30,
            DialogStyle::Warning,
            vec![button("OK", Role::Default, ok)],
        ));
        p.dialog().expect("dialog shown").close();
        testing::pump();
        assert_eq!(
            oked.borrow().len(),
            1,
            "close did not press the only button"
        );
        assert!(p.dialog().is_none());
        // Cancel and another: closing is Cancel.
        let (cancel, cancelled) = rec::<()>();
        let (go, went) = rec::<()>();
        p.render(&alert(
            31,
            DialogStyle::Warning,
            vec![
                button("Go", Role::Default, go.clone()),
                button("Cancel", Role::Cancel, cancel),
            ],
        ));
        p.dialog().expect("dialog shown").close();
        testing::pump();
        assert_eq!(cancelled.borrow().len(), 1);
        assert!(went.borrow().is_empty());
        // Two answers and no Cancel: closing picks neither.
        let (other, othered) = rec::<()>();
        p.render(&alert(
            32,
            DialogStyle::Warning,
            vec![
                button("Go", Role::Default, go),
                button("Other", Role::Normal, other),
            ],
        ));
        let d = p.dialog().expect("dialog shown");
        d.close();
        testing::pump();
        assert!(went.borrow().is_empty() && othered.borrow().is_empty());
        assert_eq!(p.dialog().as_ref(), Some(&d), "the dialog went away");
    }

    fn edit_items_act_on_what_had_the_keyboard_before_the_menu() {
        let app = gtk::Application::new(
            Some("uk.org.plinth.worktree-manager.conformance"),
            gtk::gio::ApplicationFlags::NON_UNIQUE,
        );
        app.register(gtk::gio::Cancellable::NONE)
            .expect("register the application");
        let p = Probe::with_app(&app);
        let mut v = plain();
        v.window.toolbar = vec![ToolItem::Search {
            id: "search",
            value: "hello".into(),
            placeholder: "e.g., main".into(),
            on_change: Handler::none(),
        }];
        v.menus = vec![Menu {
            role: MenuRole::Edit,
            title: "Edit".into(),
            items: vec![MenuItem::Standard(Standard::SelectAll)],
        }];
        p.render(&v);
        let search = p.search().expect("search field");
        search.grab_focus();
        testing::pump();
        search.select_region(0, 0);
        let menu = p.menu_button().expect("menu button");
        menu.popup();
        testing::pump();
        // The item chosen holds the keyboard now; Select All must still
        // reach the field.
        app.activate_action("wtm-std-select-all", None);
        testing::pump();
        menu.popdown();
        assert_eq!(
            search.selection_bounds(),
            Some((0, 5)),
            "Select All from the menu did not reach the field"
        );
    }

    /// Run the main loop until `done`, for at most a few seconds.
    fn pump_until(what: &str, done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            testing::pump();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    const PORTAL_XML: &str = r#"<node>
      <interface name="org.freedesktop.portal.Settings">
        <method name="Read">
          <arg type="s" name="namespace" direction="in"/>
          <arg type="s" name="key" direction="in"/>
          <arg type="v" name="value" direction="out"/>
        </method>
        <signal name="SettingChanged">
          <arg type="s" name="namespace"/>
          <arg type="s" name="key"/>
          <arg type="v" name="value"/>
        </signal>
      </interface>
    </node>"#;

    /// A settings portal on the session bus, played by this process. Needs a
    /// bus (`dbus-run-session -- xvfb-run -a cargo test …`); skipped without.
    fn the_appearance_follows_the_desktop_portal() {
        use gtk::{gio, glib};
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            println!("     (no session bus: skipped)");
            return;
        }
        if std::env::var_os("WTM_APPEARANCE").is_some() {
            println!("     (WTM_APPEARANCE set: skipped)");
            return;
        }
        let path = "/org/freedesktop/portal/desktop";
        let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
        let node = gio::DBusNodeInfo::for_xml(PORTAL_XML).unwrap();
        let iface = node
            .lookup_interface("org.freedesktop.portal.Settings")
            .unwrap();
        let scheme = Rc::new(std::cell::Cell::new(1u32));
        let s = scheme.clone();
        let _registration = conn
            .register_object(path, &iface)
            .method_call(move |_, _, _, _, _, _, call| {
                // `Read` wraps the value twice.
                let v = glib::Variant::from_variant(&glib::Variant::from_variant(
                    &s.get().to_variant(),
                ));
                call.return_value(Some(&glib::Variant::tuple_from_iter([v])));
            })
            .build()
            .unwrap();
        conn.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&("org.freedesktop.portal.Desktop", 4u32).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
        )
        .unwrap();
        let settings = gtk::Settings::default().unwrap();
        settings.set_gtk_application_prefer_dark_theme(false);
        let dark = || settings.is_gtk_application_prefer_dark_theme();
        // Read at launch: the desktop prefers dark.
        testing::follow_desktop_appearance();
        pump_until("dark at launch", dark);
        let change = |value: u32| {
            scheme.set(value);
            conn.emit_signal(
                None,
                path,
                "org.freedesktop.portal.Settings",
                "SettingChanged",
                Some(
                    &(
                        "org.freedesktop.appearance",
                        "color-scheme",
                        glib::Variant::from_variant(&value.to_variant()),
                    )
                        .to_variant(),
                ),
            )
            .unwrap();
        };
        // Then followed live, both ways.
        change(2);
        pump_until("light after the desktop turned light", || !dark());
        change(1);
        pump_until("dark again", dark);
        // No preference: back to GTK's own setting, which was light.
        change(0);
        pump_until("GTK's own setting with no preference", || !dark());
    }

    /// `n` cards of three worktrees each (`w:/<card>/<row>`), in a window of
    /// 900×600 that shows about two of them.
    fn tall(n: usize) -> View {
        let mut v = plain();
        v.window.frame = Some(wtm_toolkit::Frame {
            x: 0.0,
            y: 0.0,
            width: 900.0,
            height: 600.0,
        });
        v.window.list.sections = (0..n)
            .map(|i| Section {
                key: format!("r:{i}"),
                header: header(&format!("repo{i}")),
                expanded: true,
                rows: (0..3)
                    .map(|j| Row {
                        key: format!("w:/{i}/{j}"),
                        content: RowContent::Worktree(worktree(
                            &format!("branch-{i}-{j}"),
                            vec![badge("✓", Hue::Green)],
                            Handler::none(),
                        )),
                    })
                    .collect(),
            })
            .collect();
        v
    }

    fn a_saved_offset_is_restored_at_launch() {
        let p = Probe::default();
        let mut v = tall(8);
        // The row restored with the keyboard is above the restored offset:
        // giving it the keyboard must not scroll to it.
        v.window.list.selected = Some("w:/0/0".into());
        // As at launch: the effects come with the render that makes the
        // window, before it has been laid out.
        p.render_then(&v, vec![Effect::ScrollTo(362.0), Effect::FocusList]);
        pump_until("the offset restored", || {
            (p.scroll_value() - 362.0).abs() < 0.5
        });
        // And it stays there once everything has settled.
        let settle = std::time::Instant::now();
        while settle.elapsed() < std::time::Duration::from_millis(300) {
            testing::pump();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            (p.scroll_value() - 362.0).abs() < 0.5,
            "moved to {}",
            p.scroll_value()
        );
        assert_eq!(p.reported_scroll(), None, "the restore was reported");
        // The keyboard moving to a row out of view scrolls to it, and that
        // is the user's scroll.
        let far = p.row("w:/7/2").expect("last row");
        assert!(!p.shows(&far));
        far.grab_focus();
        pump_until("the focused row in view", || p.shows(&far));
        assert!(p.reported_scroll().is_some_and(|y| y > 362.0));
    }

    fn a_longer_offset_than_the_list_settles_for_its_end() {
        let p = Probe::default();
        p.render_then(&tall(3), vec![Effect::ScrollTo(100_000.0)]);
        pump_until("scrolled to the end", || {
            p.scroll_value() > 0.0 && (p.scroll_value() - p.scroll_max()).abs() < 0.5
        });
        assert_eq!(p.reported_scroll(), None);
    }

    fn the_picker_on_a_row_out_of_view_waits_for_it() {
        let p = Probe::default();
        let mut v = tall(8);
        v.window.list.selected = Some("w:/7/2".into());
        p.render(&v);
        pump_until("laid out", || p.scroll_max() > 0.0);
        let anchor = p.element("w:/7/2", BRANCH_BUTTON).expect("branch button");
        assert!(!p.shows(&anchor), "the row starts out of view");
        // Ctrl+T on it: the picker opens from a row scrolled out of view.
        let mut picker_view = picker(5, Handler::none(), Handler::none());
        picker_view.anchor = ("w:/7/2".into(), BRANCH_BUTTON);
        v.popover = Some(picker_view);
        p.render(&v);
        let entry = p.picker_entry().expect("picker");
        let popover = entry
            .ancestor(gtk::Popover::static_type())
            .and_downcast::<gtk::Popover>()
            .unwrap();
        pump_until("the popover shown", || popover.is_visible());
        assert!(p.shows(&anchor), "popped up from an anchor out of view");
        assert!(
            testing_has_focus(&p, &entry),
            "the field did not get the keyboard"
        );
    }

    fn rows_are_items_of_a_tree() {
        let p = Probe::default();
        let mut v = plain();
        v.window.list.selected = Some("w:/a".into());
        p.render(&v);
        let header = p.row("r:1").expect("header");
        let row = p.row("w:/a").expect("row");
        assert_eq!(header.accessible_role(), gtk::AccessibleRole::TreeItem);
        assert_eq!(row.accessible_role(), gtk::AccessibleRole::TreeItem);
        let mut up = row.parent();
        while let Some(w) = up.clone() {
            if w.accessible_role() == gtk::AccessibleRole::Tree {
                break;
            }
            up = w.parent();
        }
        assert!(up.is_some(), "no tree around the rows");
    }

    pub fn main() {
        let display = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())
            || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty());
        if !display || !testing::init() {
            eprintln!("conformance: no display, skipped (run under xvfb-run)");
            return;
        }
        let cases: [(&str, fn()); 14] = [
            (
                "equal rows keep their widgets",
                equal_rows_keep_their_widgets,
            ),
            (
                "a badge change keeps focus on a row button",
                a_badge_change_keeps_focus_on_a_row_button,
            ),
            (
                "a focused field is not rewritten",
                a_focused_field_is_not_rewritten,
            ),
            (
                "selection from the view is not reported",
                selection_from_the_view_is_not_reported,
            ),
            (
                "dialog buttons close, then run, and never come back",
                dialog_buttons_close_then_run_and_never_come_back,
            ),
            (
                "only the user's dismissal of the popover is reported",
                only_the_users_dismissal_of_the_popover_is_reported,
            ),
            (
                "an Info dialog opens without icon, message or body",
                an_info_dialog_opens_without_icon_message_or_body,
            ),
            ("closing a dialog is Escape", closing_a_dialog_is_escape),
            (
                "Edit items act on what had the keyboard before the menu",
                edit_items_act_on_what_had_the_keyboard_before_the_menu,
            ),
            (
                "the appearance follows the desktop's portal",
                the_appearance_follows_the_desktop_portal,
            ),
            (
                "a saved offset is restored at launch",
                a_saved_offset_is_restored_at_launch,
            ),
            (
                "a longer offset than the list settles for its end",
                a_longer_offset_than_the_list_settles_for_its_end,
            ),
            (
                "the picker on a row out of view waits for it",
                the_picker_on_a_row_out_of_view_waits_for_it,
            ),
            ("rows are items of a tree", rows_are_items_of_a_tree),
        ];
        let mut failed = 0;
        for (name, case) in cases {
            match catch_unwind(AssertUnwindSafe(case)) {
                Ok(()) => println!("ok   {name}"),
                Err(_) => {
                    println!("FAIL {name}");
                    failed += 1;
                }
            }
            // Each case leaves its windows behind; close them.
            for w in gtk::Window::list_toplevels() {
                if let Ok(w) = w.downcast::<gtk::Window>() {
                    w.destroy();
                }
            }
            testing::pump();
        }
        println!("{} passed, {failed} failed", cases.len() - failed);
        if failed > 0 {
            std::process::exit(1);
        }
    }
}
