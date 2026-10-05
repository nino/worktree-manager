//! The main window, and the one piece of state behind every window.
//!
//! Everything lives in a thread-local [`State`], borrowed for each render
//! and each native event. A notification that arrives while it is already
//! borrowed was caused by the backend itself (a field's text written during
//! a render, a window destroyed because the view dropped it), and is
//! ignored: that is how self-made changes are never reported.
//!
//! Handlers in the view only queue messages, so calling one while the state
//! is borrowed is safe; draining the queue (`wtm_toolkit::flush`) is not,
//! and happens only from the message loop.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::{
    host_event, Effect, Frame, Handler, HostEvent, Key, MainWindow, Poster, ToolItem, View,
};

use crate::controls::*;
use crate::dialog::Dialog;
use crate::list::{List, MouseEv};
use crate::look::{self, Font, Paint};
use crate::menu::Menus;
use crate::pane::Pane;
use crate::panel::Panel;
use crate::popover::Popover;
use crate::util::*;

pub const MAIN_CLASS: PCWSTR = w!("WtmMain");

// Strip heights, in DIPs.
const TOOLBAR_HEIGHT: f64 = 46.0;
const STATUS_HEIGHT: f64 = 22.0;
const NOTICE_PAD: f64 = 8.0;
const EDGE: f64 = 12.0;

static MAIN: AtomicIsize = AtomicIsize::new(0);
type Posted = Mutex<VecDeque<Box<dyn FnOnce() + Send>>>;
static POSTED: Posted = Mutex::new(VecDeque::new());

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static LATER: RefCell<VecDeque<Box<dyn FnOnce()>>> = RefCell::new(VecDeque::new());
}

pub fn main_window() -> HWND {
    hwnd(MAIN.load(Ordering::Relaxed))
}

/// Run `f` with the state, unless it is already borrowed further up the
/// stack (see the module docs) or not made yet.
pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| {
        let mut b = s.try_borrow_mut().ok()?;
        let st = b.as_mut()?;
        Some(f(st))
    })
}

/// The runtime's poster: queue the closure and wake the message loop. One
/// message per closure, so each runs on its own turn.
pub fn poster() -> Poster {
    Arc::new(|f| {
        POSTED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(f);
        unsafe {
            let _ = PostMessageW(Some(main_window()), WM_APP_RUN, WPARAM(0), LPARAM(0));
        }
    })
}

/// Run `f` on a later turn of the message loop: for work that runs a modal
/// loop of its own (the folder picker), which must never start inside a
/// render or an effect.
pub fn later(f: impl FnOnce() + 'static) {
    LATER.with(|l| l.borrow_mut().push_back(Box::new(f)));
    unsafe {
        let _ = PostMessageW(Some(main_window()), WM_APP_LATER, WPARAM(0), LPARAM(0));
    }
}

// MARK: Bindings

/// What a control does when the user works it.
#[derive(Clone)]
pub enum Bind {
    Press(Handler),
    /// A field: reports each edit.
    Text(Handler<String>),
    /// An editable combo box: typing and picks from its list.
    Combo(Handler<String>),
    /// A drop-down list.
    Choice(Handler<usize>),
    /// One segment of a segmented control, by index.
    Segment(Handler<usize>, usize),
    /// A card's chevron: the section and whether it is open now.
    Toggle(Key, bool),
    /// A worktree's branch button, which opens the picker.
    Branch(Handler),
    /// A button of the open dialog, by index.
    DialogButton(usize),
    /// The picker's list.
    PickerList(Handler<usize>),
}

/// Bindings and tooltips of every control, by handle.
#[derive(Default)]
pub struct Reg {
    pub bindings: HashMap<isize, Bind>,
    pub tips: Tips,
}

impl Reg {
    pub fn bind(&mut self, h: HWND, b: Bind) {
        self.bindings.insert(key(h), b);
    }

    pub fn unbind(&mut self, h: HWND) {
        self.bindings.remove(&key(h));
    }

    /// Destroy `h` and forget it and everything inside it.
    pub fn destroy(&mut self, h: HWND) {
        if h.is_invalid() {
            return;
        }
        let mut all = vec![h];
        unsafe {
            let _ = EnumChildWindows(
                Some(h),
                Some(collect_child),
                LPARAM(&mut all as *mut Vec<HWND> as isize),
            );
        }
        for c in &all {
            self.bindings.remove(&key(*c));
            drop_tip(&mut self.tips, *c);
            look::forget_paint(key(*c));
        }
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

unsafe extern "system" fn collect_child(h: HWND, l: LPARAM) -> windows::core::BOOL {
    let v = unsafe { &mut *(l.0 as *mut Vec<HWND>) };
    v.push(h);
    true.into()
}

// MARK: State

struct Tool {
    hwnd: HWND,
    search: bool,
}

pub struct State {
    pub main: HWND,
    pub tip: HWND,
    pub reg: Reg,
    /// The last view rendered: what native events act on.
    pub view: View,
    tools: Vec<Tool>,
    /// Ids and kinds of the toolbar's items, to tell a patch from a rebuild.
    tool_shape: Vec<String>,
    cues: HashMap<isize, String>,
    notice: Pane,
    notice_on: bool,
    empty: Pane,
    empty_on: bool,
    pub list: List,
    activity: HWND,
    pub dialog: Option<Dialog>,
    pub closed_dialogs: HashSet<u64>,
    pub popover: Option<Popover>,
    /// The anchor a click dismissed the picker from, and when: that click's
    /// own press must not open it again.
    pub swallow: Option<(isize, Instant)>,
    pub panels: Vec<Panel>,
    pub menus: Menus,
    timers: HashMap<usize, Handler>,
    next_timer: usize,
    shown: bool,
    sizing: bool,
    /// Moves made by the backend itself, not reported as frame changes.
    placing: bool,
    min_size: (f64, f64),
    placed: HashMap<isize, (RECT, bool)>,
    /// Where the keyboard was when the window lost activation.
    saved_focus: Option<HWND>,
}

impl State {
    fn new(main: HWND) -> State {
        let tip = tooltip(main);
        let pal = look::palette();
        let notice = Pane::new(main, tip, pal.notice);
        let empty = Pane::new(main, tip, pal.list);
        show(notice.hwnd, false);
        show(empty.hwnd, false);
        let list = List::new(main, tip);
        let activity = create(
            PROGRESS_CLASSW,
            "",
            WS_CHILD_ | PBS_MARQUEE_,
            WINDOW_EX_STYLE(0),
            main,
        );
        State {
            main,
            tip,
            reg: Reg::default(),
            view: View::default(),
            tools: Vec::new(),
            tool_shape: Vec::new(),
            cues: HashMap::new(),
            notice,
            notice_on: false,
            empty,
            empty_on: false,
            list,
            activity,
            dialog: None,
            closed_dialogs: HashSet::new(),
            popover: None,
            swallow: None,
            panels: Vec::new(),
            menus: Menus::default(),
            timers: HashMap::new(),
            next_timer: 100,
            shown: false,
            sizing: false,
            placing: false,
            min_size: (0.0, 0.0),
            placed: HashMap::new(),
            saved_focus: None,
        }
    }

    // MARK: Rendering

    fn render(&mut self, v: &View) {
        let w = &v.window;
        set_text_if(self.main, &w.title);
        self.min_size = w.min_size;
        self.render_toolbar(&w.toolbar);
        match &w.notice {
            Some(e) => {
                self.notice.render(e, &mut self.reg);
                self.notice_on = true;
            }
            None => self.notice_on = false,
        }
        match &w.empty {
            Some(e) => {
                self.empty.render(e, &mut self.reg);
                self.empty_on = true;
            }
            None => self.empty_on = false,
        }
        self.list.render(&w.list, &mut self.reg);
        self.render_activity(w);
        self.menus.render(self.main, &v.menus);
        self.render_dialog(v);
        self.render_popover(v);
        self.render_panels(v);
        self.view = v.clone();
        self.layout();
        if !self.shown {
            self.show_first(w);
        }
        self.keep_focus();
    }

    fn render_toolbar(&mut self, items: &[ToolItem]) {
        let shape: Vec<String> = items
            .iter()
            .map(|t| match t {
                ToolItem::Button { id, .. } => format!("button {id}"),
                ToolItem::Search { id, .. } => format!("search {id}"),
                ToolItem::Flex => "flex".into(),
            })
            .collect();
        let chrome = look::palette().chrome;
        if shape != self.tool_shape {
            for t in self.tools.drain(..) {
                self.reg.destroy(t.hwnd);
            }
            for item in items {
                match item {
                    ToolItem::Button { label, icon, .. } => {
                        let h = owner_button(
                            self.main,
                            label,
                            Paint::Tool {
                                icon: *icon,
                                label: label.clone(),
                                ground: chrome,
                            },
                        );
                        self.tools.push(Tool {
                            hwnd: h,
                            search: false,
                        });
                    }
                    ToolItem::Search { .. } => {
                        let h = create(
                            w!("EDIT"),
                            "",
                            WS_CHILD_ | WS_TABSTOP_ | ES_AUTOHSCROLL_,
                            WS_EX_CLIENTEDGE,
                            self.main,
                        );
                        set_font(h, Font::Body);
                        self.tools.push(Tool {
                            hwnd: h,
                            search: true,
                        });
                    }
                    ToolItem::Flex => {}
                }
            }
            self.tool_shape = shape;
        }
        let mut tools = self.tools.iter();
        for item in items {
            match item {
                ToolItem::Button {
                    label,
                    icon,
                    on_press,
                    ..
                } => {
                    let Some(t) = tools.next() else { break };
                    set_text_if(t.hwnd, label);
                    repaint_with(
                        t.hwnd,
                        Paint::Tool {
                            icon: *icon,
                            label: label.clone(),
                            ground: chrome,
                        },
                    );
                    set_tip(self.tip, &mut self.reg.tips, t.hwnd, Some(label));
                    self.reg.bind(t.hwnd, Bind::Press(on_press.clone()));
                }
                ToolItem::Search {
                    value,
                    placeholder,
                    on_change,
                    ..
                } => {
                    let Some(t) = tools.next() else { break };
                    set_text_if(t.hwnd, value);
                    if self.cues.get(&key(t.hwnd)) != Some(placeholder) {
                        let cue = wide(placeholder);
                        // `1`: the cue stays while the field has the keyboard
                        // and is empty, as the macOS search field's does.
                        send(t.hwnd, EM_SETCUEBANNER, 1, cue.as_ptr() as isize);
                        self.cues.insert(key(t.hwnd), placeholder.clone());
                    }
                    self.reg.bind(t.hwnd, Bind::Text(on_change.clone()));
                }
                ToolItem::Flex => {}
            }
        }
    }

    fn render_activity(&mut self, w: &MainWindow) {
        let on = w.activity.spinning;
        if visible(self.activity) != on {
            send(self.activity, PBM_SETMARQUEE, on as usize, 30);
            show(self.activity, on);
        }
        set_tip(
            self.tip,
            &mut self.reg.tips,
            self.activity,
            w.activity.tooltip.as_deref().filter(|_| on),
        );
    }

    fn render_dialog(&mut self, v: &View) {
        let want = v
            .dialogs
            .iter()
            .find(|d| !self.closed_dialogs.contains(&d.id));
        if let Some(cur) = &self.dialog {
            if want.map(|d| d.id) != Some(cur.id) {
                // Gone from the view: closed without running anything.
                let d = self.dialog.take().expect("checked above");
                self.closed_dialogs.insert(d.id);
                d.close(self.main, &mut self.reg);
            }
        }
        let Some(want) = want else { return };
        match &mut self.dialog {
            Some(d) => d.patch(want, &mut self.reg),
            None => self.dialog = Some(Dialog::open(self.main, want, &mut self.reg)),
        }
    }

    fn render_popover(&mut self, v: &View) {
        self.render_popover_window(v);
        // The anchor's hint would show over the picker's search field: the
        // pointer usually rests on the anchor, and the delay runs out after
        // the picker opens. Hints are off while a picker is open.
        send(self.tip, TTM_ACTIVATE, self.popover.is_none() as usize, 0);
    }

    fn render_popover_window(&mut self, v: &View) {
        let want = v.popover.as_ref();
        if let Some(cur) = &self.popover {
            if want.map(|p| p.id) != Some(cur.id) {
                let p = self.popover.take().expect("checked above");
                p.close(&mut self.reg);
            }
        }
        let Some(want) = want else { return };
        match &mut self.popover {
            Some(p) => p.patch(want, &mut self.reg),
            None => {
                let Some(anchor) = self.list.anchor(&want.anchor.0, want.anchor.1) else {
                    return;
                };
                self.popover = Some(Popover::open(self.main, anchor, want, &mut self.reg));
            }
        }
    }

    fn render_panels(&mut self, v: &View) {
        let mut keep = Vec::new();
        for p in std::mem::take(&mut self.panels) {
            if v.panels.iter().any(|q| q.key == p.key) {
                keep.push(p);
            } else {
                p.close(&mut self.reg);
            }
        }
        for want in &v.panels {
            match keep.iter_mut().find(|p| p.key == want.key) {
                Some(p) => p.patch(want, &mut self.reg),
                None => keep.push(Panel::open(self.main, want, &mut self.reg)),
            }
        }
        self.panels = keep;
    }

    /// The first render: the window goes where the view says, or centred at
    /// its default size, and only then is shown.
    fn show_first(&mut self, w: &MainWindow) {
        self.shown = true;
        self.placing = true;
        let s = look::scale();
        let r = match w.frame {
            Some(f) => rect(
                f.x.round() as i32,
                f.y.round() as i32,
                f.width.round() as i32,
                f.height.round() as i32,
            ),
            None => {
                let work = work_area(self.main);
                let (ww, wh) = (
                    ((980.0 * s) as i32).min(work.right - work.left),
                    ((720.0 * s) as i32).min(work.bottom - work.top),
                );
                rect(
                    work.left + (work.right - work.left - ww) / 2,
                    work.top + (work.bottom - work.top - wh) / 2,
                    ww,
                    wh,
                )
            }
        };
        unsafe {
            let _ = SetWindowPos(
                self.main,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.main, SW_SHOW);
            let _ = SetForegroundWindow(self.main);
        }
        self.placing = false;
        self.layout();
        let to = self.home_focus();
        unsafe {
            let _ = SetFocus(Some(to));
        }
    }

    /// Where the keyboard goes when nothing else has it: the empty state's
    /// button, or the list.
    fn home_focus(&self) -> HWND {
        if self.empty_on {
            if let Some(h) = pane_stops(self.empty.hwnd)
                .into_iter()
                .find(|h| visible(*h))
            {
                return h;
            }
        }
        self.list.frame
    }

    /// A focused control that a render hid hands the keyboard to the list,
    /// or keystrokes would go to a window nobody can see.
    fn keep_focus(&mut self) {
        let f = unsafe { GetFocus() };
        if f.is_invalid() || unsafe { GetAncestor(f, GA_ROOT) } != self.main {
            return;
        }
        if !visible(f) {
            let to = self.home_focus();
            unsafe {
                let _ = SetFocus(Some(to));
            }
        }
    }

    // MARK: Layout

    pub fn layout(&mut self) {
        let p = look::px;
        let c = client(self.main);
        let (width, height) = (c.right, c.bottom);
        let pal = look::palette();
        let mut bands = Vec::new();
        let mut placed = std::mem::take(&mut self.placed);
        let mut batch = Batch::new(&mut placed);

        // Toolbar: items from the left, a flex pushing the rest right.
        let bar = rect(0, 0, width, p(TOOLBAR_HEIGHT));
        bands.push((bar, pal.chrome));
        bands.push((rect(0, bar.bottom - 1, width, 1), pal.card_border));
        let mid = bar.bottom / 2;
        let widths: Vec<i32> = self
            .tools
            .iter()
            .map(|t| {
                if t.search {
                    p(240.0)
                } else {
                    tool_width(&text_of(t.hwnd))
                }
            })
            .collect();
        let total: i32 = widths.iter().sum::<i32>() + p(6.0) * self.tools.len() as i32;
        let flex = (width - 2 * p(EDGE) - total).max(0);
        let mut x = p(EDGE) - p(6.0);
        let mut tools = self.tools.iter().zip(&widths);
        for item in &self.view.window.toolbar {
            match item {
                ToolItem::Flex => x += flex,
                _ => {
                    let Some((t, w)) = tools.next() else { break };
                    let h = if t.search { p(26.0) } else { p(32.0) };
                    batch.place(t.hwnd, rect(x, mid - h / 2, *w, h));
                    x += w + p(6.0);
                }
            }
        }
        let mut top = bar.bottom;

        if self.notice_on {
            let pad = p(NOTICE_PAD);
            let inner = width - 2 * p(EDGE);
            let size = self.notice.measure(inner as f64);
            let h = size.height.ceil() as i32 + 2 * pad;
            let r = rect(0, top, width, h);
            bands.push((r, pal.notice));
            bands.push((rect(0, r.bottom - 1, width, 1), pal.card_border));
            self.notice
                .place(rect(p(EDGE), top + pad, inner, size.height.ceil() as i32));
            top = r.bottom;
        } else {
            batch.hide(self.notice.hwnd);
        }

        let status = rect(0, height - p(STATUS_HEIGHT), width, p(STATUS_HEIGHT));
        bands.push((status, pal.chrome));
        bands.push((rect(0, status.top, width, 1), pal.card_border));
        let bar_w = p(90.0);
        let bar_h = p(6.0);
        batch.set(
            self.activity,
            rect(
                width - p(EDGE) - bar_w,
                (status.top + status.bottom) / 2 - bar_h / 2,
                bar_w,
                bar_h,
            ),
            visible(self.activity),
        );

        let body = RECT {
            left: 0,
            top,
            right: width,
            bottom: status.top + 1,
        };
        if self.empty_on {
            batch.hide(self.list.frame);
            batch.apply();
            let inner = (body.right - body.left - 2 * p(40.0)).max(p(200.0));
            let size = self.empty.measure(inner as f64);
            let h = size.height.ceil() as i32;
            let w = (size.width.ceil() as i32).min(inner);
            self.empty.place(rect(
                (width - w) / 2,
                body.top + ((body.bottom - body.top - h) / 2).max(0),
                w,
                h,
            ));
            // The pane only covers its content; the list colour behind it is
            // painted as a band.
            bands.push((body, pal.list));
        } else {
            batch.hide(self.empty.hwnd);
            batch.place(
                self.list.frame,
                RECT {
                    bottom: status.top,
                    ..body
                },
            );
            batch.apply();
            self.list.layout();
        }
        self.placed = placed;
        look::with(|l| l.bands = bands);
        unsafe {
            let _ = InvalidateRect(Some(self.main), None, false);
        }
        if let Some(p) = &mut self.popover {
            p.follow();
        }
    }

    // MARK: Effects

    fn perform(&mut self, effect: Effect) {
        match effect {
            Effect::After(delay, then) => {
                let id = self.next_timer;
                self.next_timer += 1;
                self.timers.insert(id, then);
                unsafe {
                    SetTimer(
                        Some(self.main),
                        id,
                        delay.as_millis().clamp(1, u32::MAX as u128) as u32,
                        None,
                    );
                }
            }
            Effect::Copy(text) => copy_text(self.main, &text),
            Effect::PickFolders(p) => {
                let owner = p
                    .parent
                    .as_ref()
                    .and_then(|k| self.panels.iter().find(|x| &x.key == k))
                    .map(|x| x.hwnd)
                    .unwrap_or(self.main);
                later(move || {
                    let chosen = crate::dialog::pick_folders(owner, &p.title, p.multiple);
                    p.on_done.call(chosen);
                });
            }
            Effect::FocusSearch => {
                if let Some(t) = self.tools.iter().find(|t| t.search) {
                    unsafe {
                        let _ = SetFocus(Some(t.hwnd));
                    }
                    send(t.hwnd, EM_SETSEL, 0, -1);
                }
            }
            Effect::FocusList => unsafe {
                let _ = SetFocus(Some(self.list.frame));
            },
            Effect::ScrollTo(y) => self.list.scroll_to(y),
            Effect::RevealSelection => self.list.reveal_selected(),
            Effect::PresentPanel(k) => {
                if let Some(p) = self.panels.iter().find(|p| p.key == k) {
                    unsafe {
                        if IsIconic(p.hwnd).as_bool() {
                            let _ = ShowWindow(p.hwnd, SW_RESTORE);
                        }
                        let _ = SetForegroundWindow(p.hwnd);
                    }
                }
            }
        }
    }

    // MARK: The list's events

    pub fn list_key(&mut self, vk: VIRTUAL_KEY) -> bool {
        self.list.key_down(&self.view.window.list, vk)
    }

    pub fn list_mouse(&mut self, ev: MouseEv, x: i32, y: i32) {
        let tree = &self.view.window.list;
        match ev {
            MouseEv::Down => self.list.mouse_down(tree, x, y),
            MouseEv::Double => self.list.double_click(tree, x, y),
            MouseEv::Move => self.list.mouse_move(x, y),
            MouseEv::Up => self.list.mouse_up(tree, y),
            MouseEv::Lost => self.list.end_drag(tree, true),
        }
    }

    // MARK: Keyboard

    /// The main window's Tab order: the toolbar, the notice bar, then the
    /// empty state or the list and the buttons of its rows.
    fn tab_stops(&self) -> Vec<HWND> {
        let mut out: Vec<HWND> = self.tools.iter().map(|t| t.hwnd).collect();
        if self.notice_on {
            out.extend(pane_stops(self.notice.hwnd));
        }
        if self.empty_on {
            out.extend(pane_stops(self.empty.hwnd));
        } else {
            out.push(self.list.frame);
            out.extend(self.list.tab_stops());
        }
        out.retain(|h| visible(*h) && unsafe { IsWindowEnabled(*h).as_bool() });
        out
    }

    fn tab(&mut self, back: bool) {
        let stops = self.tab_stops();
        if stops.is_empty() {
            return;
        }
        let f = unsafe { GetFocus() };
        let at = stops
            .iter()
            .position(|h| *h == f || (!f.is_invalid() && unsafe { IsChild(*h, f).as_bool() }));
        let n = stops.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        let h = stops[next];
        unsafe {
            let _ = SetFocus(Some(h));
        }
        // An edit takes the keyboard with its text selected, as a dialog's do.
        if self.tools.iter().any(|t| t.search && t.hwnd == h) {
            send(h, EM_SETSEL, 0, -1);
        }
        if let Some(row) = self.list.owner_of(h) {
            self.list.reveal_control(h);
            let tree = self.view.window.list.clone();
            self.list.user_select(&tree, Some(row));
        }
        // A focus rectangle follows the keyboard from here on.
        send(
            self.main,
            WM_CHANGEUISTATE,
            ((UISF_HIDEFOCUS as usize) << 16) | UIS_CLEAR as usize,
            0,
        );
    }

    /// Keys for the main window, before they reach the focused control.
    fn main_key(&mut self, m: &MSG) -> bool {
        let vk = VIRTUAL_KEY(m.wParam.0 as u16);
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;
        if m.message != WM_KEYDOWN {
            return false;
        }
        if vk == VK_TAB && !ctrl && !alt {
            let back = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
            self.tab(back);
            return true;
        }
        if ctrl || alt {
            return false;
        }
        // Arrow keys on a row's button move the list's selection, as they
        // would from the list itself.
        if matches!(vk, VK_UP | VK_DOWN) && self.list.owner_of(m.hwnd).is_some() {
            unsafe {
                let _ = SetFocus(Some(self.list.frame));
            }
            return self.list_key(vk);
        }
        if vk == VK_ESCAPE {
            if self.list.dragging() {
                let tree = self.view.window.list.clone();
                self.list.end_drag(&tree, false);
                return true;
            }
            // Escape in the search field clears it, as on macOS.
            // The field's own change notification would arrive while the
            // state is borrowed, and be taken for one the backend made, so
            // the program is told directly and the render empties the field.
            if self.tools.iter().any(|t| t.search && t.hwnd == m.hwnd) {
                if let Some((value, on_change)) = self.view.search() {
                    if !value.is_empty() {
                        on_change.call(String::new());
                    }
                }
                return true;
            }
        }
        // Return presses the focused button, as in a dialog.
        if vk == VK_RETURN
            && matches!(
                self.reg.bindings.get(&key(m.hwnd)),
                Some(Bind::Press(_) | Bind::Branch(_) | Bind::Toggle(..))
            )
        {
            send(m.hwnd, BM_CLICK, 0, 0);
            return true;
        }
        false
    }

    fn timer(&mut self, id: usize) -> Option<Handler> {
        unsafe {
            let _ = KillTimer(Some(self.main), id);
        }
        self.timers.remove(&id)
    }

    /// The frame to remember: the restored one when maximised or
    /// minimised, which is the window to come back to.
    fn report_frame(&self) {
        if !self.shown || self.placing {
            return;
        }
        let mut wp = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if unsafe { GetWindowPlacement(self.main, &mut wp) }.is_err() {
            return;
        }
        // `rcNormalPosition` is in workspace coordinates: relative to the
        // work area of the window's monitor, not the screen.
        let r = wp.rcNormalPosition;
        let (dx, dy) = workspace_offset(self.main);
        host_event(HostEvent::FrameChanged(Frame {
            x: (r.left + dx) as f64,
            y: (r.top + dy) as f64,
            width: (r.right - r.left) as f64,
            height: (r.bottom - r.top) as f64,
        }));
    }

    /// The DPI changed: fonts, metrics and every control's size with it.
    fn rescale(&mut self, dpi: u32) {
        look::reset(dpi);
        // Every window's controls hold the old fonts. The dialog, the
        // picker and the panels are closed quietly and the render below
        // opens them again from the view, which still has them and what
        // was typed into them.
        if let Some(d) = self.dialog.take() {
            d.close(self.main, &mut self.reg);
        }
        if let Some(p) = self.popover.take() {
            p.close(&mut self.reg);
        }
        for p in std::mem::take(&mut self.panels) {
            p.close(&mut self.reg);
        }
        self.list.clear(&mut self.reg);
        for t in self.tools.drain(..) {
            self.reg.destroy(t.hwnd);
        }
        self.tool_shape.clear();
        self.cues.clear();
        let pal = look::palette();
        let notice =
            std::mem::replace(&mut self.notice, Pane::new(self.main, self.tip, pal.notice));
        notice.destroy(&mut self.reg);
        let empty = std::mem::replace(&mut self.empty, Pane::new(self.main, self.tip, pal.list));
        empty.destroy(&mut self.reg);
        show(self.notice.hwnd, false);
        show(self.empty.hwnd, false);
        self.placed.clear();
        let v = self.view.clone();
        self.render(&v);
    }
}

fn tool_width(label: &str) -> i32 {
    let icon = if look::icons_available() {
        look::px(18.0 + 6.0)
    } else {
        0
    };
    look::text_size(Font::Body, label, None, false).0 + icon + look::px(16.0)
}

/// The tab stops inside a pane, in creation order (which is its elements'
/// order).
fn pane_stops(pane: HWND) -> Vec<HWND> {
    let mut all = Vec::new();
    unsafe {
        let _ = EnumChildWindows(
            Some(pane),
            Some(collect_child),
            LPARAM(&mut all as *mut Vec<HWND> as isize),
        );
    }
    all.retain(|h| unsafe { GetWindowLongW(*h, GWL_STYLE) } as u32 & WS_TABSTOP_ != 0);
    all
}

fn monitor_info(h: HWND) -> MONITORINFO {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let m = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(m, &mut mi);
    }
    mi
}

pub fn work_area(h: HWND) -> RECT {
    monitor_info(h).rcWork
}

fn workspace_offset(h: HWND) -> (i32, i32) {
    let mi = monitor_info(h);
    (
        mi.rcWork.left - mi.rcMonitor.left,
        mi.rcWork.top - mi.rcMonitor.top,
    )
}

/// The work areas of every display, in screen pixels: what `Frame`s are
/// measured in on Windows.
pub fn screens() -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::new();
    unsafe extern "system" fn each(
        m: HMONITOR,
        _: HDC,
        _: *mut RECT,
        l: LPARAM,
    ) -> windows::core::BOOL {
        let out = unsafe { &mut *(l.0 as *mut Vec<Frame>) };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(m, &mut mi) }.as_bool() {
            let r = mi.rcWork;
            out.push(Frame {
                x: r.left as f64,
                y: r.top as f64,
                width: (r.right - r.left) as f64,
                height: (r.bottom - r.top) as f64,
            });
        }
        true.into()
    }
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

fn copy_text(owner: HWND, text: &str) {
    let w = wide(text);
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2) {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                let _ = GlobalUnlock(mem);
                // CF_UNICODETEXT. On success the clipboard owns the memory.
                let _ = SetClipboardData(13, Some(HANDLE(mem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

// MARK: The backend

/// What the runtime holds: every call goes to the thread's [`State`].
pub struct Backend;

impl wtm_toolkit::Backend for Backend {
    fn render(&self, view: &View) {
        if with_state(|s| s.render(view)).is_none() {
            log::error!("render while the window state was in use");
        }
    }

    fn perform(&self, effect: Effect) {
        with_state(|s| s.perform(effect));
    }
}

/// Make the state for `main`, once.
pub fn install(main: HWND) {
    MAIN.store(key(main), Ordering::Relaxed);
    let st = State::new(main);
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

// MARK: Commands

/// A control's notification (`WM_COMMAND` from any of the app's windows).
pub fn command(ctl: HWND, code: u32) {
    let Some(Some(bind)) = with_state(|s| s.reg.bindings.get(&key(ctl)).cloned()) else {
        return;
    };
    match bind {
        Bind::Press(h) if code == BN_CLICKED => h.call(()),
        Bind::Text(h) if code == EN_CHANGE => h.call(text_of(ctl)),
        Bind::Combo(h) => match code {
            CBN_EDITCHANGE => h.call(text_of(ctl)),
            CBN_SELCHANGE => {
                // The edit still holds the old text while this is sent; the
                // pick is read from the list.
                let i = send(ctl, CB_GETCURSEL, 0, 0);
                if i >= 0 {
                    h.call(combo_item(ctl, i as usize));
                }
            }
            _ => {}
        },
        Bind::Choice(h) if code == CBN_SELCHANGE => {
            let i = send(ctl, CB_GETCURSEL, 0, 0);
            if i >= 0 {
                h.call(i as usize);
            }
        }
        Bind::Segment(h, i) if code == BN_CLICKED => h.call(i),
        Bind::Toggle(k, open) if code == BN_CLICKED => {
            with_state(|s| {
                let tree = s.view.window.list.clone();
                s.list.user_select(&tree, Some(k.clone()));
                tree.on_toggle.call((k, !open));
            });
        }
        Bind::Branch(h) if code == BN_CLICKED => {
            let swallowed = with_state(|s| match s.swallow.take() {
                Some((a, t)) => a == key(ctl) && t.elapsed().as_millis() < 600,
                None => false,
            })
            .unwrap_or(false);
            if !swallowed {
                with_state(|s| {
                    let tree = s.view.window.list.clone();
                    if let Some(row) = s.list.owner_of(ctl) {
                        s.list.user_select(&tree, Some(row));
                    }
                });
                h.call(());
            }
        }
        Bind::DialogButton(i) if code == BN_CLICKED => crate::dialog::press(i),
        Bind::PickerList(h) if code == LBN_SELCHANGE => {
            let i = send(ctl, LB_GETCURSEL, 0, 0);
            if i >= 0 {
                h.call(i as usize);
            }
        }
        _ => {}
    }
}

fn combo_item(ctl: HWND, i: usize) -> String {
    let len = send(ctl, CB_GETLBTEXTLEN, i, 0);
    if len < 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    send(ctl, CB_GETLBTEXT, i, buf.as_mut_ptr() as isize);
    String::from_utf16_lossy(&buf[..len as usize])
}

// MARK: The message loop

/// Handle `m` before it is dispatched: the keys the app owns, dialog
/// navigation, accelerators. `true`: it was handled.
pub fn pre_translate(m: &MSG) -> bool {
    let root = unsafe { GetAncestor(m.hwnd, GA_ROOT) };
    let main = main_window();
    let is_key = (WM_KEYFIRST..=WM_KEYLAST).contains(&m.message);
    if !is_key || root.is_invalid() {
        return false;
    }
    if crate::popover::is_popover(root) {
        return false;
    }
    if crate::dialog::is_dialog(root) {
        return crate::dialog::pre_translate(root, m);
    }
    if root != main {
        // A panel: the menu's shortcuts work there too, then dialog keys.
        if accelerate(m) {
            return true;
        }
        return unsafe { IsDialogMessageW(root, m) }.as_bool();
    }
    if accelerate(m) {
        return true;
    }
    with_state(|s| s.main_key(m)).unwrap_or(false)
}

fn accelerate(m: &MSG) -> bool {
    if m.message != WM_KEYDOWN && m.message != WM_SYSKEYDOWN {
        return false;
    }
    let Some(accel) = with_state(|s| s.menus.accel()).flatten() else {
        return false;
    };
    unsafe { TranslateAcceleratorW(main_window(), accel, m) != 0 }
}

/// Write everything down and leave.
pub fn quit() -> ! {
    host_event(HostEvent::WillQuit);
    wtm_toolkit::flush();
    std::process::exit(0)
}

// MARK: The main window's procedure

pub unsafe extern "system" fn main_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = container_msg(h, msg, w, l) {
        return r;
    }
    match msg {
        WM_APP_RUN => {
            let f = POSTED.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
            if let Some(f) = f {
                f();
            }
            LRESULT(0)
        }
        WM_APP_LATER => {
            let f = LATER.with(|q| q.borrow_mut().pop_front());
            if let Some(f) = f {
                f();
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if let Some(Some(then)) = with_state(|s| s.timer(w.0)) {
                then.call(());
            }
            LRESULT(0)
        }
        WM_INITMENUPOPUP => {
            // The menu's enabled states must reflect every message already
            // sent (Ctrl+N straight after an arrow key).
            wtm_toolkit::flush();
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = loword(w.0) as u32;
            wtm_toolkit::flush();
            crate::menu::command(id);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            let dc = HDC(w.0 as *mut _);
            let bands = look::with(|lk| lk.bands.clone());
            let all = client(h);
            look::fill(dc, &all, look::palette().window);
            for (r, c) in bands {
                look::fill(dc, &r, c);
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                let dc = BeginPaint(h, &mut ps);
                let bands = look::with(|lk| lk.bands.clone());
                for (r, c) in bands {
                    look::fill(dc, &r, c);
                }
                let _ = EndPaint(h, &ps);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            with_state(|s| {
                s.layout();
                if !s.sizing && w.0 as u32 != SIZE_MINIMIZED {
                    s.report_frame();
                }
            });
            LRESULT(0)
        }
        WM_MOVE => {
            with_state(|s| {
                if !s.sizing {
                    s.report_frame();
                }
            });
            LRESULT(0)
        }
        WM_ENTERSIZEMOVE => {
            with_state(|s| s.sizing = true);
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            with_state(|s| {
                s.sizing = false;
                s.report_frame();
            });
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mm = unsafe { &mut *(l.0 as *mut MINMAXINFO) };
            if let Some((mw, mh)) = with_state(|s| s.min_size) {
                let s = look::scale();
                let mut r = rect(0, 0, (mw * s) as i32, (mh * s) as i32);
                unsafe {
                    let _ =
                        AdjustWindowRectEx(&mut r, WS_OVERLAPPEDWINDOW, true, WINDOW_EX_STYLE(0));
                }
                mm.ptMinTrackSize.x = r.right - r.left;
                mm.ptMinTrackSize.y = r.bottom - r.top;
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = unsafe { &*(l.0 as *const RECT) };
            with_state(|s| {
                s.placing = true;
                unsafe {
                    let _ = SetWindowPos(
                        h,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                s.placing = false;
                s.rescale(hiword(w.0) as u32);
            });
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
            // Colours and the message font follow the system's. Of the
            // many settings changes, only the one for the message font
            // (`SPI_SETNONCLIENTMETRICS`) matters here.
            if msg != WM_SETTINGCHANGE || w.0 == SPI_SETNONCLIENTMETRICS.0 as usize {
                with_state(|s| s.rescale(unsafe { GetDpiForWindow(h) }));
            }
            unsafe { DefWindowProcW(h, msg, w, l) }
        }
        WM_ACTIVATE => {
            let active = loword(w.0) as u32 != WA_INACTIVE;
            with_state(|s| {
                if active {
                    if let Some(f) = s.saved_focus.take() {
                        if unsafe { IsWindow(Some(f)) }.as_bool() && visible(f) {
                            unsafe {
                                let _ = SetFocus(Some(f));
                            }
                        }
                    }
                } else {
                    let f = unsafe { GetFocus() };
                    if !f.is_invalid() && unsafe { IsChild(h, f) }.as_bool() {
                        s.saved_focus = Some(f);
                    }
                }
            });
            LRESULT(0)
        }
        WM_ACTIVATEAPP => {
            if w.0 != 0 {
                host_event(HostEvent::Activated);
            }
            LRESULT(0)
        }
        WM_NCACTIVATE => {
            // The picker takes activation to read the keyboard; the window
            // it hangs from keeps its active title bar meanwhile.
            let keep = w.0 == 0 && crate::popover::is_open();
            unsafe { DefWindowProcW(h, msg, WPARAM(if keep { 1 } else { w.0 }), l) }
        }
        WM_CLOSE => quit(),
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if w.0 != 0 {
                host_event(HostEvent::WillQuit);
                wtm_toolkit::flush();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}
