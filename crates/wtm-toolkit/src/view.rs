//! The vocabulary: what the UI asks a toolkit to show, as plain values.
//!
//! A [`View`] is rebuilt from the program's state after every batch of
//! messages and handed to the backend, which brings its native widgets in
//! line with it. Nothing here is a widget; everything is data a backend can
//! compare with the last `View` it rendered, so an unchanged row costs a
//! comparison and nothing more.
//!
//! The vocabulary is generic in mechanism — stacks, text, buttons, fields,
//! a sectioned list, dialogs, menus — and covers exactly what this app
//! draws: its icon set, its badge hues, its text styles. Backends map each
//! item to the native equivalent and make no decisions about the app.

use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Identity of a list section, row, dialog or panel: stable across renders.
pub type Key = String;

/// Identity of an element within its row, dialog or panel: what a popover
/// is anchored to and what keyboard focus is put on.
pub type Id = &'static str;

// MARK: Handlers

/// A callback in a [`View`]. Built by the program (see
/// [`ViewCx`](crate::ViewCx)), it queues a message when called, so calling
/// it from inside a native callback never re-enters the program.
///
/// Handlers always compare equal: a `View` is compared to find what to
/// redraw, and what a button does is never drawn. So a backend takes the new
/// handlers of every widget on every render, even one whose row compared
/// equal: a handler captures what its row showed when it was built, and an
/// old one would act on that.
pub struct Handler<A = ()>(Rc<dyn Fn(A)>);

impl<A> Handler<A> {
    pub fn new(f: impl Fn(A) + 'static) -> Self {
        Handler(Rc::new(f))
    }

    /// A handler that does nothing.
    pub fn none() -> Self {
        Handler(Rc::new(|_| {}))
    }

    pub fn call(&self, arg: A) {
        (self.0)(arg)
    }
}

impl<A> Clone for Handler<A> {
    fn clone(&self) -> Self {
        Handler(self.0.clone())
    }
}

impl<A> PartialEq for Handler<A> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<A> fmt::Debug for Handler<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Handler")
    }
}

impl<A> Default for Handler<A> {
    fn default() -> Self {
        Handler::none()
    }
}

// MARK: The whole UI

/// Everything on screen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    pub window: MainWindow,
    pub menus: Vec<Menu>,
    /// Modal questions, in the order they were asked. Only the first is on
    /// screen; the rest wait for it to close, as sheets queue on macOS.
    pub dialogs: Vec<Dialog>,
    pub popover: Option<Popover>,
    /// Secondary windows, such as Settings.
    pub panels: Vec<Panel>,
}

/// A window's frame in screen coordinates, as the window manager reports
/// it. On macOS the origin is the bottom-left corner of the main screen; a
/// toolkit that cannot place windows (GTK 4) reports and uses only the size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MainWindow {
    pub title: String,
    pub min_size: (f64, f64),
    /// Where the window opens. Read once, when it is created; `None` centres
    /// it at its default size.
    pub frame: Option<Frame>,
    pub toolbar: Vec<ToolItem>,
    /// The one-line bar under the toolbar, when there is something to say.
    pub notice: Option<Element>,
    /// Shown in place of the list when there is nothing to list.
    pub empty: Option<Element>,
    pub list: TreeList,
    /// The small spinner in the bottom corner (refresh and fetch).
    pub activity: Spinner,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolItem {
    Button {
        id: Id,
        label: String,
        icon: Icon,
        on_press: Handler,
    },
    Search {
        id: Id,
        value: String,
        placeholder: String,
        on_change: Handler<String>,
    },
    /// Pushes what follows to the far end.
    Flex,
}

// MARK: The list

/// A list of collapsible sections, each a header row over its own rows —
/// the repo cards. Rows are keyed, so a backend can tell a row that moved,
/// came or went from one that only changed.
///
/// Rows are typed rather than built from [`Element`]s: a backend keeps one
/// native view per row and patches it in place, so focus, an open popover's
/// anchor and a tooltip under the pointer survive a status change. Heights
/// and the card drawing are the backend's.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeList {
    pub sections: Vec<Section>,
    /// The row with the keyboard's cursor: a section's key or a row's key.
    /// Two-way: the backend reports the user's changes through `on_select`,
    /// and sets the native selection only when it differs from this. A
    /// selection the backend set itself is not reported.
    pub selected: Option<Key>,
    pub on_select: Handler<Option<Key>>,
    /// The user opened (`true`) or closed a section. Not reported for
    /// sections the backend opened or closed to match the view.
    pub on_toggle: Handler<(Key, bool)>,
    /// A section was dragged and dropped before another (`None`: at the
    /// end), both among the sections rendered. A drop beside the dragged
    /// section itself is not reported. `None`: sections cannot be dragged.
    pub on_reorder: Option<Handler<Reorder>>,
    /// Space on the selected row.
    pub on_activate: Handler<Key>,
}

/// A drop: the dragged section's key, and the key of the section it lands
/// before, or `None` for the end.
pub type Reorder = (Key, Option<Key>);

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub key: Key,
    pub header: RepoHeader,
    /// Open. A backend shows `rows` only when this is set.
    pub expanded: bool,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub key: Key,
    pub content: RowContent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowContent {
    Worktree(WorktreeRow),
    Pending(PendingRow),
}

/// A repo card's header: name and summary, path, and the repo's buttons.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoHeader {
    pub name: String,
    /// "main · 3 worktrees".
    pub meta: String,
    /// The path as shown (abbreviated with `~`), and in full for its tooltip.
    pub path: String,
    pub path_full: String,
    /// Why the repo could not be listed, in red after the path.
    pub error: Option<String>,
    /// A listing is on its way: the header's spinner turns.
    pub loading: bool,
    pub can_create: bool,
    pub on_new_worktree: Handler,
    pub on_settings: Handler,
    pub on_copy_path: Handler,
}

/// The id of a worktree row's branch button, for a popover's anchor.
pub const BRANCH_BUTTON: Id = "branch";

/// One worktree: its branch button and badges over its path and actions.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeRow {
    /// The branch as drawn, an agent prefix as a mark; "(detached)" when
    /// there is no branch.
    pub branch: Rich,
    /// The whole name, for assistive technology and the tooltip.
    pub branch_name: String,
    /// The branch button opens the picker.
    pub can_switch: bool,
    pub switch_hint: String,
    pub on_switch: Handler,
    /// `None` hides the copy-branch button (detached head).
    pub on_copy_branch: Option<Handler>,
    pub badges: Vec<Badge>,
    /// An operation is running: the spinner turns and this is shown beside it.
    pub busy: Option<String>,
    pub path: String,
    pub path_full: String,
    pub on_copy_path: Handler,
    /// The icon buttons at the end of the second line, in groups with a
    /// wider gap between them.
    pub actions: Vec<Vec<RowAction>>,
}

/// An icon button in a row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowAction {
    pub id: Id,
    pub icon: Icon,
    pub hint: String,
    pub enabled: bool,
    pub hidden: bool,
    pub tint: Tint,
    pub on_press: Handler,
}

/// A worktree being created, or one whose creation failed.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingRow {
    pub branch: String,
    /// Why it failed; `None` while it is still being created.
    pub error: Option<String>,
    pub on_dismiss: Handler,
}

// MARK: Elements

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    Stack(Stack),
    Text(Text),
    Button(Button),
    Badge(Badge),
    Spinner(Spinner),
    Field(Field),
    Combo(Combo),
    Segmented(Segmented),
    Choice(Choice),
    /// A scrolling, selectable, monospaced block of text.
    TextBlock(String),
    Form(Form),
    /// Spacing after the previous child of a stack, in place of the stack's
    /// own spacing.
    Gap(f64),
    /// Takes the leftover length of a stack, pushing what follows to its end.
    Flex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// How a stack places its children across its axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    /// First baselines line up (horizontal stacks).
    Baseline,
    /// Children stretch across the stack.
    Fill,
}

/// What gives way first when a row is too narrow. Elements shrink in order:
/// `First`, then `Second`, then everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shrink {
    #[default]
    Normal,
    First,
    Second,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
    pub axis: Axis,
    pub spacing: f64,
    pub align: Align,
    pub children: Vec<Element>,
    pub shrink: Shrink,
    /// Takes extra length along its parent's axis.
    pub grow: bool,
    pub hidden: bool,
}

impl Stack {
    pub fn row(spacing: f64) -> Self {
        Stack {
            axis: Axis::Horizontal,
            spacing,
            align: Align::Center,
            children: Vec::new(),
            shrink: Shrink::Normal,
            grow: false,
            hidden: false,
        }
    }

    pub fn column(spacing: f64) -> Self {
        Stack {
            axis: Axis::Vertical,
            align: Align::Start,
            ..Stack::row(spacing)
        }
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn shrink(mut self, shrink: Shrink) -> Self {
        self.shrink = shrink;
        self
    }

    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
        self.children.extend(children);
        self
    }
}

/// Text with some runs emphasised and agent marks in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rich {
    pub spans: Vec<Span>,
}

impl Rich {
    pub fn plain(text: impl Into<String>) -> Self {
        Rich {
            spans: vec![Span::Text {
                text: text.into(),
                strong: false,
            }],
        }
    }

    /// The text without marks or emphasis.
    pub fn to_plain(&self) -> String {
        self.spans
            .iter()
            .filter_map(|s| match s {
                Span::Text { text, .. } => Some(text.as_str()),
                Span::Mark(_) => None,
            })
            .collect()
    }
}

impl From<&str> for Rich {
    fn from(s: &str) -> Self {
        Rich::plain(s)
    }
}

impl From<String> for Rich {
    fn from(s: String) -> Self {
        Rich::plain(s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Span {
    Text { text: String, strong: bool },
    Mark(Mark),
}

/// The marks drawn in place of an agent's branch prefix (see [`crate::marks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Claude,
    Cursor,
}

/// The app's text styles. Sizes are macOS points; other toolkits scale them
/// to their own default size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    /// 13: dialog captions and ordinary text.
    Body,
    /// 16, semibold: the empty state's title.
    Heading,
    /// 14, semibold: a repo's name.
    Title,
    /// 12: the notice bar, the empty state's line.
    Small,
    /// 11: counts, notes, hints, statuses.
    Caption,
    /// 10.5, monospaced: paths.
    Path,
    /// 12, monospaced: branch names.
    Branch,
    /// 12, monospaced, semibold: a branch being created.
    BranchStrong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ink {
    #[default]
    Primary,
    Secondary,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Leading,
    Trailing,
    Center,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub id: Option<Id>,
    pub content: Rich,
    pub style: TextStyle,
    pub ink: Ink,
    pub align: TextAlign,
    pub tooltip: Option<String>,
    /// At most this many lines, the last truncated; 0 for no limit.
    pub lines: u32,
    /// Wraps to the width it is given instead of asking for its full length.
    pub wrap: bool,
    pub selectable: bool,
    pub shrink: Shrink,
    pub grow: bool,
    pub hidden: bool,
}

impl Text {
    pub fn new(content: impl Into<Rich>) -> Self {
        Text {
            id: None,
            content: content.into(),
            style: TextStyle::Body,
            ink: Ink::Primary,
            align: TextAlign::Leading,
            tooltip: None,
            lines: 1,
            wrap: false,
            selectable: false,
            shrink: Shrink::Normal,
            grow: false,
            hidden: false,
        }
    }

    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn ink(mut self, ink: Ink) -> Self {
        self.ink = ink;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn lines(mut self, lines: u32) -> Self {
        self.lines = lines;
        self
    }

    /// Wrap, with no limit on the lines: a limit goes after this.
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self.lines = 0;
        self
    }

    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    pub fn shrink(mut self, shrink: Shrink) -> Self {
        self.shrink = shrink;
        self
    }

    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }
}

/// The app's icons. Each backend maps them to its platform's own set (SF
/// Symbols, the freedesktop icon theme, the Segoe icon font).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Add,
    Refresh,
    Settings,
    Copy,
    Push,
    Pull,
    Merge,
    Editor,
    Terminal,
    Folder,
    Delete,
    Close,
    /// Up-and-down chevrons: "this opens a list".
    Chevrons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// A standard push button.
    Push,
    /// A small push button, for a row's main action.
    Small,
    /// A small, quiet button for a bar ("Details…").
    Accessory,
    /// A borderless glyph; `hint` says what it does.
    Icon,
    /// The branch switcher: a small bezelled button whose label is a branch
    /// name, with chevrons after it.
    Pill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tint {
    #[default]
    Normal,
    /// Destructive: red where the platform tints such things.
    Danger,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub id: Id,
    pub kind: ButtonKind,
    pub label: Rich,
    pub icon: Option<Icon>,
    /// Tooltip, and the accessibility help. Icon buttons must have one.
    pub hint: Option<String>,
    /// What assistive technology reads as the name, when the label does not
    /// say it all (a mark standing in for a prefix).
    pub a11y_label: Option<String>,
    pub enabled: bool,
    pub hidden: bool,
    pub tint: Tint,
    /// Return presses it.
    pub is_default: bool,
    pub shrink: Shrink,
    pub on_press: Handler,
}

impl Button {
    pub fn new(id: Id, kind: ButtonKind, label: impl Into<Rich>, on_press: Handler) -> Self {
        Button {
            id,
            kind,
            label: label.into(),
            icon: None,
            hint: None,
            a11y_label: None,
            enabled: true,
            hidden: false,
            tint: Tint::Normal,
            is_default: false,
            shrink: Shrink::Normal,
            on_press,
        }
    }

    pub fn icon(id: Id, icon: Icon, hint: impl Into<String>, on_press: Handler) -> Self {
        Button {
            icon: Some(icon),
            hint: Some(hint.into()),
            ..Button::new(id, ButtonKind::Icon, Rich::default(), on_press)
        }
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    pub fn tint(mut self, tint: Tint) -> Self {
        self.tint = tint;
        self
    }

    pub fn default_button(mut self) -> Self {
        self.is_default = true;
        self
    }

    pub fn shrink(mut self, shrink: Shrink) -> Self {
        self.shrink = shrink;
        self
    }
}

/// The colour family of a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hue {
    Green,
    Blue,
    Orange,
    Purple,
    Teal,
    Red,
    Yellow,
    Gray,
    Accent,
}

/// How loud a badge is where colour is rationed (the dark appearance on
/// macOS): only `Attention` and `Alarm` spend colour there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Quiet,
    Notable,
    Attention,
    Alarm,
}

/// A small rounded status label.
#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    pub text: String,
    pub hue: Hue,
    pub emphasis: Emphasis,
    pub tooltip: String,
}

/// An indeterminate progress spinner. One that is not spinning draws
/// nothing but keeps its place, so a row does not shift when it starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spinner {
    pub spinning: bool,
    pub tooltip: Option<String>,
}

/// A one-line text field.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub id: Id,
    pub value: String,
    pub placeholder: String,
    /// A backend never disables a field that has the keyboard: something
    /// landing mid-edit must not take it away.
    pub enabled: bool,
    pub on_change: Handler<String>,
}

/// A text field with a list of suggestions it completes from.
#[derive(Debug, Clone, PartialEq)]
pub struct Combo {
    pub id: Id,
    pub value: String,
    pub options: Vec<String>,
    pub placeholder: String,
    pub enabled: bool,
    pub on_change: Handler<String>,
}

/// A row of mutually exclusive segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Segmented {
    pub id: Id,
    pub options: Vec<String>,
    pub selected: usize,
    pub on_select: Handler<usize>,
}

/// A pop-up list of options.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub id: Id,
    pub options: Vec<String>,
    pub selected: usize,
    pub a11y_label: Option<String>,
    pub on_select: Handler<usize>,
}

/// Captions in one column, controls in the next.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Form {
    pub rows: Vec<FormRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormRow {
    /// Empty for a row whose content lines up under the controls (a hint, a
    /// note) with no caption of its own.
    pub caption: String,
    pub content: Element,
    pub hidden: bool,
}

impl Form {
    pub fn row(mut self, caption: impl Into<String>, content: impl Into<Element>) -> Self {
        self.rows.push(FormRow {
            caption: caption.into(),
            content: content.into(),
            hidden: false,
        });
        self
    }

    pub fn row_hidden(
        mut self,
        caption: impl Into<String>,
        content: impl Into<Element>,
        hidden: bool,
    ) -> Self {
        self.rows.push(FormRow {
            caption: caption.into(),
            content: content.into(),
            hidden,
        });
        self
    }

    /// A line of explanation under the control above it.
    pub fn hint(self, text: impl Into<String>) -> Self {
        let hint = Text::new(text.into())
            .style(TextStyle::Caption)
            .ink(Ink::Secondary)
            .wrap();
        self.row("", hint)
    }
}

macro_rules! element_from {
    ($($t:ident),*) => {$(
        impl From<$t> for Element {
            fn from(v: $t) -> Self {
                Element::$t(v)
            }
        }
    )*};
}
element_from!(Stack, Text, Button, Badge, Spinner, Field, Combo, Segmented, Choice, Form);

// MARK: Dialogs, popovers, panels

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogStyle {
    #[default]
    Info,
    Warning,
    Critical,
}

/// A modal question. Every button closes it and then runs its handler;
/// Escape runs the `Cancel` button's. A backend shows a given `id` once: one
/// that leaves the view is closed, and one it closed is never shown again.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    pub id: u64,
    pub title: String,
    pub message: String,
    pub style: DialogStyle,
    pub body: Option<Element>,
    /// In order of importance: the first is the default (Return).
    pub buttons: Vec<DialogButton>,
    /// The field that has the keyboard when it opens.
    pub focus: Option<Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Default,
    Cancel,
    Destructive,
    Normal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DialogButton {
    pub label: String,
    pub role: Role,
    pub enabled: bool,
    pub on_press: Handler,
}

/// A transient list under one element of a row: the branch picker.
#[derive(Debug, Clone, PartialEq)]
pub struct Popover {
    /// Opened afresh when this changes.
    pub id: u64,
    /// The row and the element in it the popover hangs from.
    pub anchor: (Key, Id),
    pub list: FilterList,
}

/// A filter field over a list it narrows. ↑ and ↓ in the field move the
/// selection, Return chooses, Escape or a click elsewhere dismisses. A click
/// on the anchor while it is open dismisses it and is not passed on.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterList {
    pub query: String,
    pub placeholder: String,
    pub items: Vec<ListItem>,
    pub selected: Option<usize>,
    pub on_query: Handler<String>,
    /// −1 for up, +1 for down.
    pub on_move: Handler<i32>,
    pub on_choose: Handler<usize>,
    pub on_dismiss: Handler,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub label: Rich,
    pub checked: bool,
}

/// A secondary window. Closing it runs `on_close`.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub key: Key,
    pub title: String,
    pub body: Element,
    pub focus: Option<Id>,
    pub on_close: Handler,
}

// MARK: Menus

/// Where a menu belongs. Toolkits without an application menu fold `App`'s
/// items into the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRole {
    App,
    File,
    Edit,
    Window,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub role: MenuRole,
    pub title: String,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    Action {
        label: String,
        shortcut: Option<Shortcut>,
        enabled: bool,
        on_select: Handler,
    },
    Separator,
    /// An item the platform implements itself; skipped where it has none.
    Standard(Standard),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    CloseWindow,
    Minimize,
    Zoom,
    BringAllToFront,
}

/// A key with the platform's primary modifier (⌘ on macOS, Ctrl
/// elsewhere), plus these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: KeyName,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    pub fn primary(key: KeyName) -> Self {
        Shortcut {
            key,
            shift: false,
            alt: false,
        }
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyName {
    Char(char),
    Up,
    Down,
}

// MARK: Effects and host events

/// A one-off command, issued from `update` and performed after the render
/// that follows it.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Run `then` after `delay`, on the main thread.
    After(Duration, Handler),
    /// Put text on the clipboard.
    Copy(String),
    PickFolders(PickFolders),
    /// Put the keyboard in the toolbar's search field.
    FocusSearch,
    /// Put the keyboard on the list, so the arrow keys move its selection.
    FocusList,
    /// Scroll the list to this offset from the top, clamped, once it has
    /// laid out the rows of this render.
    ScrollTo(f64),
    /// Scroll the selected row into view.
    RevealSelection,
    /// Bring a panel forward.
    PresentPanel(Key),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PickFolders {
    pub title: String,
    pub multiple: bool,
    /// The panel the picker belongs to, or `None` for the main window.
    pub parent: Option<Key>,
    /// The chosen folders; empty when cancelled.
    pub on_done: Handler<Vec<PathBuf>>,
}

/// What the toolkit tells the program that no widget in the view asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// The event loop is running; `screens` are the usable areas of the
    /// displays, in the coordinates `Frame` uses.
    Started { screens: Vec<Frame> },
    /// The app came to the front.
    Activated,
    /// The app is about to exit: last chance to write anything.
    WillQuit,
    /// The system asked the app to open these (a folder dropped on the Dock
    /// icon).
    OpenPaths(Vec<PathBuf>),
    /// The main window moved or was resized. In full screen this is the
    /// frame from before it, which is the window to come back to.
    FrameChanged(Frame),
    /// The list scrolled; the offset of its top from the first row.
    Scrolled(f64),
}

// MARK: Looking things up (tests, and backends finding an anchor)

impl Element {
    /// Every element in this tree, depth first, this one included.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Element)) {
        f(self);
        match self {
            Element::Stack(s) => {
                for c in &s.children {
                    c.walk(f);
                }
            }
            Element::Form(form) => {
                for r in &form.rows {
                    r.content.walk(f);
                }
            }
            _ => {}
        }
    }

    pub fn button(&self, id: Id) -> Option<&Button> {
        let mut found = None;
        self.walk(&mut |e| {
            if let Element::Button(b) = e {
                if b.id == id && found.is_none() {
                    found = Some(b);
                }
            }
        });
        found
    }

    pub fn field(&self, id: Id) -> Option<&Field> {
        let mut found = None;
        self.walk(&mut |e| {
            if let Element::Field(x) = e {
                if x.id == id && found.is_none() {
                    found = Some(x);
                }
            }
        });
        found
    }

    pub fn combo(&self, id: Id) -> Option<&Combo> {
        let mut found = None;
        self.walk(&mut |e| {
            if let Element::Combo(x) = e {
                if x.id == id && found.is_none() {
                    found = Some(x);
                }
            }
        });
        found
    }

    pub fn segmented(&self, id: Id) -> Option<&Segmented> {
        let mut found = None;
        self.walk(&mut |e| {
            if let Element::Segmented(x) = e {
                if x.id == id && found.is_none() {
                    found = Some(x);
                }
            }
        });
        found
    }

    pub fn choice(&self, id: Id) -> Option<&Choice> {
        let mut found = None;
        self.walk(&mut |e| {
            if let Element::Choice(x) = e {
                if x.id == id && found.is_none() {
                    found = Some(x);
                }
            }
        });
        found
    }

    pub fn text(&self, id: Id) -> Option<&Text> {
        let mut found = None;
        self.walk(&mut |e| {
            if let Element::Text(x) = e {
                if x.id == Some(id) && found.is_none() {
                    found = Some(x);
                }
            }
        });
        found
    }

    /// Every visible text, badge and button label, in order: what a reader
    /// of the row would see.
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_texts(&mut out);
        out
    }

    fn collect_texts(&self, out: &mut Vec<String>) {
        match self {
            Element::Stack(s) if !s.hidden => {
                for c in &s.children {
                    c.collect_texts(out);
                }
            }
            Element::Form(f) => {
                for r in f.rows.iter().filter(|r| !r.hidden) {
                    if !r.caption.is_empty() {
                        out.push(r.caption.clone());
                    }
                    r.content.collect_texts(out);
                }
            }
            Element::Text(t) if !t.hidden => out.push(t.content.to_plain()),
            Element::Badge(b) => out.push(b.text.clone()),
            Element::Button(b) if !b.hidden && b.kind != ButtonKind::Icon => {
                out.push(b.label.to_plain())
            }
            _ => {}
        }
    }
}

impl TreeList {
    pub fn section(&self, key: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.key == key)
    }

    pub fn row(&self, key: &str) -> Option<&Row> {
        self.sections
            .iter()
            .flat_map(|s| s.rows.iter())
            .find(|r| r.key == key)
    }

    pub fn worktree(&self, key: &str) -> Option<&WorktreeRow> {
        match self.row(key).map(|r| &r.content) {
            Some(RowContent::Worktree(w)) => Some(w),
            _ => None,
        }
    }

    /// Keys of the rows on screen, in order: each header, then its rows if
    /// it is open.
    pub fn visible_keys(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for s in &self.sections {
            out.push(s.key.as_str());
            if s.expanded {
                out.extend(s.rows.iter().map(|r| r.key.as_str()));
            }
        }
        out
    }
}

impl WorktreeRow {
    pub fn action(&self, id: Id) -> Option<&RowAction> {
        self.actions.iter().flatten().find(|a| a.id == id)
    }
}

impl View {
    /// The menu item labelled `label`, in any menu.
    pub fn menu_item(&self, label: &str) -> Option<(&Handler, bool)> {
        self.menus
            .iter()
            .flat_map(|m| m.items.iter())
            .find_map(|i| match i {
                MenuItem::Action {
                    label: l,
                    enabled,
                    on_select,
                    ..
                } if l == label => Some((on_select, *enabled)),
                _ => None,
            })
    }

    pub fn toolbar_button(&self, id: Id) -> Option<&Handler> {
        self.window.toolbar.iter().find_map(|t| match t {
            ToolItem::Button {
                id: i, on_press, ..
            } if *i == id => Some(on_press),
            _ => None,
        })
    }

    pub fn search(&self) -> Option<(&str, &Handler<String>)> {
        self.window.toolbar.iter().find_map(|t| match t {
            ToolItem::Search {
                value, on_change, ..
            } => Some((value.as_str(), on_change)),
            _ => None,
        })
    }

    pub fn panel(&self, key: &str) -> Option<&Panel> {
        self.panels.iter().find(|p| p.key == key)
    }
}

impl Dialog {
    pub fn button(&self, label: &str) -> Option<&DialogButton> {
        self.buttons.iter().find(|b| b.label == label)
    }
}
