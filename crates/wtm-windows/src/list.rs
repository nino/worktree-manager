//! The repo cards: a scrolling frame holding one tall content window, whose
//! children are real controls for every row (buttons take the keyboard,
//! assistive technology reads them, tooltips hang off them) and whose own
//! painting is only the cards behind them.
//!
//! Scrolling moves the content window inside the frame, so no control ever
//! moves for it. Controls are kept per section and row key and patched in
//! place; a collapsed card's rows are hidden, not destroyed.

use std::collections::{HashMap, HashSet};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wtm_toolkit::{
    host_event, HostEvent, Icon, Key, PendingRow, RepoHeader, Rich, RowContent, Tint, TreeList,
    WorktreeRow, BRANCH_BUTTON,
};

use crate::access;
use crate::app::{with_state, Bind, Reg};
use crate::controls::*;
use crate::look::{self, Card, Font, Paint};
use crate::util::*;

pub const FRAME_CLASS: windows::core::PCWSTR = w!("WtmListFrame");
pub const CONTENT_CLASS: windows::core::PCWSTR = w!("WtmListContent");

// Geometry in DIPs, after the macOS cards (`rowview.rs`), a little taller
// for Windows' controls.
const CARD_GAP: f64 = 12.0;
const CARD_MARGIN: f64 = 14.0;
const CARD_RADIUS: f64 = 8.0;
const HEADER_HEIGHT: f64 = 54.0;
const PLATE_INSET: f64 = 10.0;
const PLATE_RADIUS: f64 = 6.0;
const PLATE_SPACING: f64 = 8.0;
const WELL_PAD: f64 = 10.0;
const WORKTREE_HEIGHT: f64 = 58.0;
const PENDING_HEIGHT: f64 = 40.0;
const CARD_BOTTOM_ROOM: f64 = 4.0;
const PAD: f64 = 12.0;

struct HeaderCtl {
    chevron: HWND,
    name: HWND,
    meta: HWND,
    spinner: HWND,
    path: HWND,
    copy: HWND,
    error: HWND,
    new_worktree: HWND,
    settings: HWND,
    last: Option<RepoHeader>,
}

struct SectionCtl {
    key: Key,
    expanded: bool,
    header: HeaderCtl,
    rows: Vec<Key>,
}

struct WorktreeCtl {
    pill: HWND,
    copy_branch: HWND,
    spinner: HWND,
    busy: HWND,
    badges: Vec<HWND>,
    path: HWND,
    copy_path: HWND,
    actions: Vec<ActionCtl>,
    last: Option<WorktreeRow>,
}

/// A row's icon button, with what its placement needs to know.
struct ActionCtl {
    id: &'static str,
    hwnd: HWND,
    icon: Icon,
    hidden: bool,
    /// The first of its group: a wider gap goes before it.
    group_start: bool,
}

struct PendingCtl {
    branch: HWND,
    spinner: HWND,
    status: HWND,
    dismiss: HWND,
    last: Option<PendingRow>,
}

enum RowCtl {
    Worktree(Box<WorktreeCtl>),
    Pending(PendingCtl),
}

impl RowCtl {
    fn controls(&self) -> Vec<HWND> {
        match self {
            RowCtl::Worktree(w) => {
                let mut v = vec![
                    w.pill,
                    w.copy_branch,
                    w.spinner,
                    w.busy,
                    w.path,
                    w.copy_path,
                ];
                v.extend(&w.badges);
                v.extend(w.actions.iter().map(|a| a.hwnd));
                v
            }
            RowCtl::Pending(p) => vec![p.branch, p.spinner, p.status, p.dismiss],
        }
    }

    /// The controls Tab stops on, in order.
    fn tab_stops(&self) -> Vec<HWND> {
        match self {
            RowCtl::Worktree(w) => {
                let mut v = vec![w.pill, w.copy_branch, w.copy_path];
                v.extend(w.actions.iter().filter(|a| !a.hidden).map(|a| a.hwnd));
                v
            }
            RowCtl::Pending(p) => vec![p.dismiss],
        }
    }
}

impl HeaderCtl {
    fn controls(&self) -> Vec<HWND> {
        vec![
            self.chevron,
            self.name,
            self.meta,
            self.spinner,
            self.path,
            self.copy,
            self.error,
            self.new_worktree,
            self.settings,
        ]
    }
}

/// A press on a card that may become a drag.
struct Press {
    key: Key,
    x: i32,
    y: i32,
}

pub struct List {
    pub frame: HWND,
    pub content: HWND,
    tip: HWND,
    sections: Vec<SectionCtl>,
    rows: HashMap<Key, RowCtl>,
    /// Which row or header each control belongs to.
    owner: HashMap<isize, Key>,
    placed: HashMap<isize, (RECT, bool)>,
    /// Every row on screen with its top and bottom, in order.
    extents: Vec<(Key, i32, i32)>,
    /// What assistive technology is told each row is called, by key.
    names: HashMap<Key, String>,
    /// Every row on screen as an outline item: its card-wide rectangle,
    /// whether it is a header, and whether that is open.
    items: Vec<(Key, RECT, bool, bool)>,
    height: i32,
    scroll: i32,
    pub selected: Option<Key>,
    press: Option<Press>,
    dragging: Option<Key>,
}

impl List {
    pub fn new(parent: HWND, tip: HWND) -> List {
        let frame = create(
            FRAME_CLASS,
            "Repositories",
            WS_CHILD_ | WS_VISIBLE_ | WS_TABSTOP_ | WS_VSCROLL.0 | WS_CLIPCHILDREN.0,
            WINDOW_EX_STYLE(0),
            parent,
        );
        let content = create(
            CONTENT_CLASS,
            "",
            WS_CHILD_ | WS_VISIBLE_ | WS_CLIPCHILDREN.0,
            WINDOW_EX_STYLE(0),
            frame,
        );
        List {
            frame,
            content,
            tip,
            sections: Vec::new(),
            rows: HashMap::new(),
            owner: HashMap::new(),
            placed: HashMap::new(),
            extents: Vec::new(),
            names: HashMap::new(),
            items: Vec::new(),
            height: 0,
            scroll: 0,
            selected: None,
            press: None,
            dragging: None,
        }
    }

    /// Forget every row's controls, so the next render builds them afresh
    /// (the DPI changed and fonts with it).
    pub fn clear(&mut self, reg: &mut Reg) {
        for s in self.sections.drain(..) {
            for h in s.header.controls() {
                reg.destroy(h);
            }
        }
        for (_, r) in self.rows.drain() {
            for h in r.controls() {
                reg.destroy(h);
            }
        }
        self.owner.clear();
        self.placed.clear();
    }

    // MARK: Rendering

    pub fn render(&mut self, tree: &TreeList, reg: &mut Reg) {
        let mut old: HashMap<Key, SectionCtl> = self
            .sections
            .drain(..)
            .map(|s| (s.key.clone(), s))
            .collect();
        let mut rows_seen: HashSet<Key> = HashSet::new();
        let mut sections = Vec::with_capacity(tree.sections.len());
        for sec in &tree.sections {
            let mut ctl = match old.remove(&sec.key) {
                Some(c) => c,
                None => SectionCtl {
                    key: sec.key.clone(),
                    expanded: sec.expanded,
                    header: self.make_header(&sec.key),
                    rows: Vec::new(),
                },
            };
            ctl.expanded = sec.expanded;
            self.patch_header(&mut ctl.header, &sec.key, &sec.header, sec.expanded, reg);
            ctl.rows = sec.rows.iter().map(|r| r.key.clone()).collect();
            for row in &sec.rows {
                rows_seen.insert(row.key.clone());
                let fits = matches!(
                    (self.rows.get(&row.key), &row.content),
                    (Some(RowCtl::Worktree(_)), RowContent::Worktree(_))
                        | (Some(RowCtl::Pending(_)), RowContent::Pending(_))
                );
                if !fits {
                    if let Some(gone) = self.rows.remove(&row.key) {
                        self.drop_controls(gone.controls(), reg);
                    }
                    let made = match &row.content {
                        RowContent::Worktree(_) => {
                            RowCtl::Worktree(Box::new(self.make_worktree(&row.key)))
                        }
                        RowContent::Pending(_) => RowCtl::Pending(self.make_pending(&row.key)),
                    };
                    self.rows.insert(row.key.clone(), made);
                }
                let mut ctl_row = self.rows.remove(&row.key).expect("made above");
                match (&mut ctl_row, &row.content) {
                    (RowCtl::Worktree(c), RowContent::Worktree(w)) => {
                        self.patch_worktree(c, &row.key, w, reg)
                    }
                    (RowCtl::Pending(c), RowContent::Pending(p)) => self.patch_pending(c, p, reg),
                    _ => {}
                }
                self.rows.insert(row.key.clone(), ctl_row);
            }
            sections.push(ctl);
        }
        for (_, gone) in old {
            self.drop_controls(gone.header.controls(), reg);
        }
        let stale: Vec<Key> = self
            .rows
            .keys()
            .filter(|k| !rows_seen.contains(*k))
            .cloned()
            .collect();
        for k in stale {
            if let Some(gone) = self.rows.remove(&k) {
                self.drop_controls(gone.controls(), reg);
            }
        }
        self.sections = sections;
        self.names = access_names(tree);
        // The view's selection is shown, never reported back.
        if self.selected != tree.selected {
            self.selected = tree.selected.clone();
        }
        self.layout();
    }

    fn drop_controls(&mut self, controls: Vec<HWND>, reg: &mut Reg) {
        // A control about to go that has the keyboard hands it to the list,
        // or Windows would leave it with nothing.
        if controls.iter().any(|h| has_focus(*h)) {
            unsafe {
                let _ = SetFocus(Some(self.frame));
            }
        }
        for h in controls {
            self.owner.remove(&key(h));
            self.placed.remove(&key(h));
            reg.destroy(h);
        }
    }

    fn own(&mut self, h: HWND, row: &Key) -> HWND {
        self.owner.insert(key(h), row.clone());
        h
    }

    fn make_header(&mut self, k: &Key) -> HeaderCtl {
        let pal = look::palette();
        let g = pal.band;
        let c = self.content;
        let lbl = |t: &str, f: Font, style: u32, ink: Color| {
            let h = label(c, t, f, style, g, ink);
            forward_mouse(h);
            h
        };
        let header = HeaderCtl {
            chevron: owner_button(
                c,
                "",
                Paint::Disclosure {
                    open: true,
                    ground: g,
                },
            ),
            name: lbl(
                "",
                Font::Title,
                SS_LEFTNOWORDWRAP_ | SS_ENDELLIPSIS_,
                pal.text,
            ),
            meta: lbl(
                "",
                Font::Caption,
                SS_LEFTNOWORDWRAP_ | SS_ENDELLIPSIS_,
                pal.secondary,
            ),
            spinner: spinner(c, g),
            path: lbl(
                "",
                Font::Path,
                SS_LEFTNOWORDWRAP_ | SS_PATHELLIPSIS_,
                pal.secondary,
            ),
            copy: owner_button(c, "Copy path", icon_paint(Icon::Copy, g, Tint::Normal)),
            error: lbl(
                "",
                Font::Caption,
                SS_LEFTNOWORDWRAP_ | SS_ENDELLIPSIS_,
                pal.red,
            ),
            new_worktree: push_button(c, "New Worktree", Font::Small, g),
            settings: owner_button(
                c,
                "Repo settings",
                icon_paint(Icon::Settings, g, Tint::Normal),
            ),
            last: None,
        };
        for h in header.controls() {
            self.own(h, k);
        }
        header
    }

    fn patch_header(
        &mut self,
        c: &mut HeaderCtl,
        k: &Key,
        h: &RepoHeader,
        expanded: bool,
        reg: &mut Reg,
    ) {
        let tip = self.tip;
        if c.last.as_ref() != Some(h) {
            set_text_if(c.name, &h.name);
            set_text_if(c.meta, &h.meta);
            set_text_if(c.path, &h.path);
            set_text_if(c.error, h.error.as_deref().unwrap_or(""));
            enable(c.new_worktree, h.can_create);
            set_spinning(c.spinner, h.loading);
            set_tip(tip, &mut reg.tips, c.path, Some(&h.path_full));
            set_tip(tip, &mut reg.tips, c.error, h.error.as_deref());
            set_tip(
                tip,
                &mut reg.tips,
                c.spinner,
                h.loading.then_some("Listing worktrees…"),
            );
            set_tip(tip, &mut reg.tips, c.copy, Some("Copy path"));
            set_tip(tip, &mut reg.tips, c.settings, Some("Repo settings"));
            c.last = Some(h.clone());
        }
        set_text_if(
            c.chevron,
            &format!(
                "{} {}",
                if expanded { "Collapse" } else { "Expand" },
                h.name
            ),
        );
        repaint_with(
            c.chevron,
            Paint::Disclosure {
                open: expanded,
                ground: look::palette().band,
            },
        );
        reg.bind(c.chevron, Bind::Toggle(k.clone(), expanded));
        reg.bind(c.copy, Bind::Press(h.on_copy_path.clone()));
        reg.bind(c.new_worktree, Bind::Press(h.on_new_worktree.clone()));
        reg.bind(c.settings, Bind::Press(h.on_settings.clone()));
    }

    fn make_worktree(&mut self, k: &Key) -> WorktreeCtl {
        let pal = look::palette();
        let g = pal.plate;
        let c = self.content;
        let busy = label(c, "", Font::Caption, SS_LEFTNOWORDWRAP_, g, pal.secondary);
        forward_mouse(busy);
        let path = label(
            c,
            "",
            Font::Path,
            SS_LEFTNOWORDWRAP_ | SS_PATHELLIPSIS_,
            g,
            pal.secondary,
        );
        forward_mouse(path);
        let ctl = WorktreeCtl {
            pill: owner_button(
                c,
                "",
                Paint::Pill {
                    rich: Rich::default(),
                    ground: g,
                },
            ),
            copy_branch: owner_button(
                c,
                "Copy branch name",
                icon_paint(Icon::Copy, g, Tint::Normal),
            ),
            spinner: spinner(c, g),
            busy,
            badges: Vec::new(),
            path,
            copy_path: owner_button(c, "Copy path", icon_paint(Icon::Copy, g, Tint::Normal)),
            actions: Vec::new(),
            last: None,
        };
        for h in [
            ctl.pill,
            ctl.copy_branch,
            ctl.spinner,
            ctl.busy,
            ctl.path,
            ctl.copy_path,
        ] {
            self.own(h, k);
        }
        ctl
    }

    fn patch_worktree(&mut self, c: &mut WorktreeCtl, k: &Key, w: &WorktreeRow, reg: &mut Reg) {
        let tip = self.tip;
        let g = look::palette().plate;
        // Badges: reuse what is there, add or drop the difference.
        while c.badges.len() > w.badges.len() {
            let h = c.badges.pop().expect("longer than wanted");
            self.drop_controls(vec![h], reg);
        }
        while c.badges.len() < w.badges.len() {
            let h = owner_label(
                self.content,
                "",
                Paint::Badge {
                    text: String::new(),
                    hue: wtm_toolkit::Hue::Gray,
                    ground: g,
                },
            );
            forward_mouse(h);
            self.own(h, k);
            c.badges.push(h);
        }
        for (h, b) in c.badges.iter().zip(&w.badges) {
            set_text_if(*h, &b.text);
            repaint_with(
                *h,
                Paint::Badge {
                    text: b.text.clone(),
                    hue: b.hue,
                    ground: g,
                },
            );
            set_tip(tip, &mut reg.tips, *h, Some(&b.tooltip));
        }
        // Action buttons, keyed by id so each keeps its control.
        let wanted: Vec<_> = w
            .actions
            .iter()
            .flat_map(|g| g.iter().enumerate().map(|(i, a)| (i == 0, a)))
            .collect();
        let ids: Vec<&str> = wanted.iter().map(|a| a.1.id).collect();
        let have: Vec<&str> = c.actions.iter().map(|a| a.id).collect();
        if ids != have {
            let gone: Vec<HWND> = c.actions.drain(..).map(|a| a.hwnd).collect();
            self.drop_controls(gone, reg);
            for (_, a) in &wanted {
                let h = owner_button(self.content, &a.hint, icon_paint(a.icon, g, a.tint));
                self.own(h, k);
                c.actions.push(ActionCtl {
                    id: a.id,
                    hwnd: h,
                    icon: a.icon,
                    hidden: a.hidden,
                    group_start: false,
                });
            }
        }
        for (ctl, (first, a)) in c.actions.iter_mut().zip(&wanted) {
            let h = ctl.hwnd;
            ctl.icon = a.icon;
            ctl.hidden = a.hidden;
            ctl.group_start = *first;
            set_text_if(h, &a.hint);
            repaint_with(h, icon_paint(a.icon, g, a.tint));
            enable(h, a.enabled);
            set_tip(tip, &mut reg.tips, h, Some(&a.hint));
            reg.bind(h, Bind::Press(a.on_press.clone()));
        }
        if c.last.as_ref() != Some(w) {
            // The pill's text is the whole branch name: what assistive
            // technology reads, though a mark is drawn for the prefix.
            set_text_if(c.pill, &w.branch_name);
            repaint_with(
                c.pill,
                Paint::Pill {
                    rich: w.branch.clone(),
                    ground: g,
                },
            );
            enable(c.pill, w.can_switch);
            set_tip(tip, &mut reg.tips, c.pill, Some(&w.switch_hint));
            set_tip(tip, &mut reg.tips, c.copy_branch, Some("Copy branch name"));
            set_spinning(c.spinner, w.busy.is_some());
            set_text_if(c.busy, w.busy.as_deref().unwrap_or(""));
            set_text_if(c.path, &w.path);
            set_tip(tip, &mut reg.tips, c.path, Some(&w.path_full));
            set_tip(tip, &mut reg.tips, c.copy_path, Some("Copy path"));
            c.last = Some(w.clone());
        }
        reg.bind(c.pill, Bind::Branch(w.on_switch.clone()));
        match &w.on_copy_branch {
            Some(h) => reg.bind(c.copy_branch, Bind::Press(h.clone())),
            None => reg.unbind(c.copy_branch),
        }
        reg.bind(c.copy_path, Bind::Press(w.on_copy_path.clone()));
    }

    fn make_pending(&mut self, k: &Key) -> PendingCtl {
        let pal = look::palette();
        let g = pal.plate;
        let c = self.content;
        let branch = label(
            c,
            "",
            Font::BranchBold,
            SS_LEFTNOWORDWRAP_ | SS_ENDELLIPSIS_,
            g,
            pal.text,
        );
        let status = label(
            c,
            "",
            Font::Caption,
            SS_LEFTNOWORDWRAP_ | SS_ENDELLIPSIS_,
            g,
            pal.secondary,
        );
        forward_mouse(branch);
        forward_mouse(status);
        let ctl = PendingCtl {
            branch,
            spinner: spinner(c, g),
            status,
            dismiss: owner_button(c, "Dismiss", icon_paint(Icon::Close, g, Tint::Normal)),
            last: None,
        };
        for h in [ctl.branch, ctl.spinner, ctl.status, ctl.dismiss] {
            self.own(h, k);
        }
        ctl
    }

    fn patch_pending(&mut self, c: &mut PendingCtl, p: &PendingRow, reg: &mut Reg) {
        let pal = look::palette();
        if c.last.as_ref() != Some(p) {
            set_text_if(c.branch, &p.branch);
            set_spinning(c.spinner, p.error.is_none());
            set_text_if(c.status, p.error.as_deref().unwrap_or("Creating…"));
            set_paint_ink(
                c.status,
                if p.error.is_some() {
                    pal.red
                } else {
                    pal.secondary
                },
            );
            invalidate(c.status);
            set_tip(self.tip, &mut reg.tips, c.status, p.error.as_deref());
            set_tip(self.tip, &mut reg.tips, c.dismiss, Some("Dismiss"));
            c.last = Some(p.clone());
        }
        reg.bind(c.dismiss, Bind::Press(p.on_dismiss.clone()));
    }

    // MARK: Layout

    /// Place every control and work out the cards, then size the content
    /// window and the scroll bar to them.
    pub fn layout(&mut self) {
        for _ in 0..2 {
            let width = client(self.frame).right;
            self.layout_at(width);
            // Showing or hiding the scroll bar changes the width: once more
            // at the new one.
            if client(self.frame).right == width {
                break;
            }
        }
    }

    fn layout_at(&mut self, width: i32) {
        let p = look::px;
        let mut placed = std::mem::take(&mut self.placed);
        let mut batch = Batch::new(&mut placed);
        let mut cards = Vec::with_capacity(self.sections.len());
        let mut extents = Vec::new();
        let mut items = Vec::new();
        let x = p(CARD_MARGIN);
        let cw = (width - 2 * x).max(p(200.0));
        let mut y = 0;
        for sec in &self.sections {
            y += p(CARD_GAP);
            let top = y;
            let band = rect(x, top, cw, p(HEADER_HEIGHT));
            place_header(&mut batch, &sec.header, band);
            extents.push((sec.key.clone(), top, band.bottom));
            items.push((sec.key.clone(), band, true, sec.expanded));
            y = band.bottom;
            let mut plates = Vec::new();
            let open = sec.expanded && !sec.rows.is_empty();
            if open {
                y += p(WELL_PAD);
            }
            for (i, rk) in sec.rows.iter().enumerate() {
                let Some(row) = self.rows.get(rk) else {
                    continue;
                };
                if !sec.expanded {
                    for h in row.controls() {
                        batch.hide(h);
                    }
                    continue;
                }
                if i > 0 {
                    y += p(PLATE_SPACING);
                }
                let h = match row {
                    RowCtl::Worktree(_) => p(WORKTREE_HEIGHT),
                    RowCtl::Pending(_) => p(PENDING_HEIGHT),
                };
                let plate = rect(x + p(PLATE_INSET), y, cw - 2 * p(PLATE_INSET), h);
                match row {
                    RowCtl::Worktree(c) => place_worktree(&mut batch, c, plate),
                    RowCtl::Pending(c) => place_pending(&mut batch, c, plate),
                }
                extents.push((rk.clone(), plate.top, plate.bottom));
                items.push((rk.clone(), plate, false, false));
                plates.push((rk.clone(), plate));
                y += h;
            }
            if open {
                y += p(WELL_PAD);
            }
            cards.push(Card {
                key: sec.key.clone(),
                rect: RECT {
                    left: x,
                    top,
                    right: x + cw,
                    bottom: y,
                },
                band_bottom: band.bottom,
                plates,
            });
            y += p(CARD_BOTTOM_ROOM);
        }
        y += p(PAD);
        batch.apply();
        self.placed = placed;
        self.extents = extents;
        self.items = items;
        self.height = y;
        let selected = self.selected.clone();
        let dragging = self.dragging.clone();
        look::with(|l| {
            l.list.cards = cards;
            l.list.selected = selected;
            l.list.dragging = dragging;
        });
        self.sync_scroll(width);
        self.publish_access();
    }

    /// Hand assistive technology the rows as they now stand.
    fn publish_access(&self) {
        let items = self
            .items
            .iter()
            .map(|(k, rect, header, expanded)| access::Item {
                name: self.names.get(k).cloned().unwrap_or_else(|| k.clone()),
                rect: *rect,
                header: *header,
                expanded: *expanded,
            })
            .collect();
        let selected = self
            .selected
            .as_ref()
            .and_then(|s| self.items.iter().position(|i| &i.0 == s));
        access::publish(self.frame, self.content, items, selected);
    }

    fn view_height(&self) -> i32 {
        client(self.frame).bottom
    }

    fn sync_scroll(&mut self, width: i32) {
        let view = self.view_height();
        let max = (self.height - view).max(0);
        self.scroll = self.scroll.clamp(0, max);
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: self.height.max(1) - 1,
            nPage: view.max(0) as u32,
            nPos: self.scroll,
            nTrackPos: 0,
        };
        unsafe {
            SetScrollInfo(self.frame, SB_VERT, &info, true);
            let _ = SetWindowPos(
                self.content,
                None,
                0,
                -self.scroll,
                width,
                self.height.max(view),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        invalidate(self.content);
    }

    /// Scroll to `y` pixels from the top, clamped. `report`: the user did
    /// it, so the program hears of it.
    pub fn set_scroll(&mut self, y: i32, report: bool) {
        let max = (self.height - self.view_height()).max(0);
        let y = y.clamp(0, max);
        if y == self.scroll {
            return;
        }
        self.scroll = y;
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_POS,
            nPos: y,
            ..Default::default()
        };
        unsafe {
            SetScrollInfo(self.frame, SB_VERT, &info, true);
            let _ = SetWindowPos(
                self.content,
                None,
                0,
                -y,
                0,
                0,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
            );
        }
        if report {
            host_event(HostEvent::Scrolled(y as f64 / look::scale()));
        }
        // The picker hangs from a row's button, which just moved. The
        // state is borrowed here; the picker follows on the next turn.
        if crate::popover::is_open() {
            unsafe {
                let _ = PostMessageW(
                    Some(crate::app::main_window()),
                    WM_APP_FOLLOW,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    /// `Effect::ScrollTo`: an offset in DIPs, as `Scrolled` reports them.
    pub fn scroll_to(&mut self, dips: f64) {
        self.set_scroll((dips * look::scale()).round() as i32, false);
    }

    fn reveal(&mut self, top: i32, bottom: i32) {
        let margin = look::px(CARD_GAP);
        let view = self.view_height();
        if top - margin < self.scroll {
            self.set_scroll(top - margin, true);
        } else if bottom + margin > self.scroll + view {
            self.set_scroll(bottom + margin - view, true);
        }
    }

    pub fn reveal_selected(&mut self) {
        let Some(k) = &self.selected else { return };
        if let Some(&(_, top, bottom)) = self.extents.iter().find(|e| &e.0 == k) {
            self.reveal(top, bottom);
        }
    }

    /// Scroll a row control into view (Tab reached it).
    pub fn reveal_control(&mut self, h: HWND) {
        if let Some((r, true)) = self.placed.get(&key(h)).copied() {
            self.reveal(r.top, r.bottom);
        }
    }

    // MARK: Keyboard and selection

    /// The row or header a control is part of.
    pub fn owner_of(&self, h: HWND) -> Option<Key> {
        self.owner.get(&key(h)).cloned()
    }

    /// The controls Tab stops on, in order: each card's header buttons,
    /// then its rows' if it is open. Only those shown and enabled.
    pub fn tab_stops(&self) -> Vec<HWND> {
        let mut out = Vec::new();
        for s in &self.sections {
            let h = &s.header;
            out.extend([h.chevron, h.copy, h.new_worktree, h.settings]);
            if s.expanded {
                for rk in &s.rows {
                    if let Some(r) = self.rows.get(rk) {
                        out.extend(r.tab_stops());
                    }
                }
            }
        }
        out.retain(|h| visible(*h) && unsafe { IsWindowEnabled(*h).as_bool() });
        out
    }

    /// The branch button of the row `row`, for the picker to hang from.
    pub fn anchor(&self, row: &str, id: &str) -> Option<HWND> {
        if id != BRANCH_BUTTON {
            return None;
        }
        match self.rows.get(row)? {
            RowCtl::Worktree(w) if visible(w.pill) => Some(w.pill),
            _ => None,
        }
    }

    /// A selection the user made: shown at once and reported.
    pub fn user_select(&mut self, tree: &TreeList, k: Option<Key>) {
        if self.selected == k {
            return;
        }
        self.selected = k.clone();
        let shown = k.clone();
        look::with(|l| l.list.selected = shown);
        invalidate(self.content);
        self.reveal_selected();
        self.publish_access();
        tree.on_select.call(k);
    }

    /// A key pressed on the list. `true` when it was the list's.
    pub fn key_down(&mut self, tree: &TreeList, vk: VIRTUAL_KEY) -> bool {
        let keys: Vec<Key> = tree.visible_keys().into_iter().map(String::from).collect();
        let at = self
            .selected
            .as_ref()
            .and_then(|s| keys.iter().position(|k| k == s));
        let section = |k: &str| tree.section(k);
        match vk {
            VK_DOWN | VK_UP | VK_HOME | VK_END => {
                if keys.is_empty() {
                    return true;
                }
                let next = match (vk, at) {
                    (VK_HOME, _) => 0,
                    (VK_END, _) => keys.len() - 1,
                    (VK_DOWN, None) => 0,
                    (VK_UP, None) => keys.len() - 1,
                    (VK_DOWN, Some(i)) => (i + 1).min(keys.len() - 1),
                    (_, Some(i)) => i.saturating_sub(1),
                    (_, None) => 0,
                };
                self.user_select(tree, Some(keys[next].clone()));
                true
            }
            VK_LEFT | VK_RIGHT => {
                let Some(sel) = self.selected.clone() else {
                    return true;
                };
                if let Some(s) = section(&sel) {
                    let open = vk == VK_RIGHT;
                    if s.expanded != open {
                        tree.on_toggle.call((sel, open));
                    }
                } else if vk == VK_LEFT {
                    // From a row, Left goes to its card's header.
                    if let Some(s) = tree
                        .sections
                        .iter()
                        .find(|s| s.rows.iter().any(|r| r.key == sel))
                    {
                        self.user_select(tree, Some(s.key.clone()));
                    }
                }
                true
            }
            VK_SPACE => {
                if let Some(sel) = self.selected.clone() {
                    tree.on_activate.call(sel);
                }
                true
            }
            VK_ESCAPE if self.dragging.is_some() => {
                self.end_drag(tree, false);
                true
            }
            _ => false,
        }
    }

    // MARK: Mouse

    fn hit(&self, x: i32, y: i32) -> Option<(Key, bool)> {
        look::with(|l| {
            for c in &l.list.cards {
                if y < c.rect.top || y >= c.rect.bottom || x < c.rect.left || x >= c.rect.right {
                    continue;
                }
                if y < c.band_bottom {
                    return Some((c.key.clone(), true));
                }
                for (k, r) in &c.plates {
                    if y >= r.top && y < r.bottom && x >= r.left && x < r.right {
                        return Some((k.clone(), false));
                    }
                }
                return None;
            }
            None
        })
    }

    pub fn mouse_down(&mut self, tree: &TreeList, x: i32, y: i32) {
        unsafe {
            let _ = SetFocus(Some(self.frame));
        }
        let hit = self.hit(x, y);
        self.user_select(tree, hit.as_ref().map(|h| h.0.clone()));
        self.press = match hit {
            Some((k, true)) if tree.on_reorder.is_some() => Some(Press { key: k, x, y }),
            _ => None,
        };
    }

    pub fn double_click(&mut self, tree: &TreeList, x: i32, y: i32) {
        if let Some((k, true)) = self.hit(x, y) {
            if let Some(s) = tree.section(&k) {
                tree.on_toggle.call((k, !s.expanded));
            }
        }
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        if self.dragging.is_none() {
            let Some(p) = &self.press else { return };
            let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            if (x - p.x).abs() <= dx && (y - p.y).abs() <= dy {
                return;
            }
            self.dragging = Some(p.key.clone());
            unsafe { SetCapture(self.content) };
        }
        // Near an edge of the frame, the list scrolls under the drag.
        let in_view = y - self.scroll;
        let edge = look::px(24.0);
        if in_view < edge {
            self.set_scroll(self.scroll - look::px(12.0), true);
        } else if in_view > self.view_height() - edge {
            self.set_scroll(self.scroll + look::px(12.0), true);
        }
        let line = self.drop_at(y).map(|(_, line)| line);
        let dragging = self.dragging.clone();
        look::with(|l| {
            l.list.drop_line = line;
            l.list.dragging = dragging;
        });
        invalidate(self.content);
    }

    /// Where a card dropped at `y` lands: before which section (`None`:
    /// the end), and the y of the line that shows it. `None` for a drop
    /// beside the dragged card itself, which would move nothing.
    fn drop_at(&self, y: i32) -> Option<(Option<Key>, i32)> {
        let dragged = self.dragging.as_ref()?;
        let cards: Vec<(Key, i32, i32)> = look::with(|l| {
            l.list
                .cards
                .iter()
                .map(|c| (c.key.clone(), c.rect.top, c.rect.bottom))
                .collect()
        });
        let from = cards.iter().position(|c| &c.0 == dragged)?;
        let gap = cards.iter().filter(|c| (c.1 + c.2) / 2 < y).count();
        if gap == from || gap == from + 1 {
            return None;
        }
        let half = look::px(CARD_GAP) / 2;
        let line = match cards.get(gap) {
            Some(c) => c.1 - half,
            None => cards.last().map_or(0, |c| c.2 + half),
        };
        Some((cards.get(gap).map(|c| c.0.clone()), line))
    }

    pub fn mouse_up(&mut self, tree: &TreeList, y: i32) {
        self.press = None;
        if self.dragging.is_some() {
            let target = self.drop_at(y);
            let dragged = self.dragging.clone();
            self.end_drag(tree, false);
            if let (Some((before, _)), Some(k), Some(on_reorder)) =
                (target, dragged, tree.on_reorder.as_ref())
            {
                on_reorder.call((k, before));
            }
        }
    }

    /// Stop dragging; `lost`: the capture was taken away, so it is not
    /// released again.
    pub fn end_drag(&mut self, _tree: &TreeList, lost: bool) {
        self.press = None;
        if self.dragging.take().is_none() {
            return;
        }
        look::with(|l| {
            l.list.drop_line = None;
            l.list.dragging = None;
        });
        if !lost {
            unsafe {
                let _ = ReleaseCapture();
            }
        }
        invalidate(self.content);
    }

    pub fn dragging(&self) -> bool {
        self.dragging.is_some()
    }

    pub fn wheel(&mut self, delta: i16) {
        let step = look::px(60.0);
        self.set_scroll(self.scroll - delta as i32 * step / 120, true);
    }

    pub fn vscroll(&mut self, code: u16) {
        let view = self.view_height();
        let line = look::px(40.0);
        let y = match SCROLLBAR_COMMAND(code as i32) {
            SB_LINEUP => self.scroll - line,
            SB_LINEDOWN => self.scroll + line,
            SB_PAGEUP => self.scroll - view,
            SB_PAGEDOWN => self.scroll + view,
            SB_TOP => 0,
            SB_BOTTOM => self.height,
            SB_THUMBTRACK | SB_THUMBPOSITION => {
                let mut info = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_TRACKPOS,
                    ..Default::default()
                };
                unsafe {
                    let _ = GetScrollInfo(self.frame, SB_VERT, &mut info);
                }
                info.nTrackPos
            }
            _ => return,
        };
        self.set_scroll(y, true);
    }
}

fn icon_paint(icon: Icon, ground: Color, tint: Tint) -> Paint {
    Paint::Icon {
        icon,
        ground,
        tint,
        small: true,
    }
}

fn text_w(h: HWND, f: Font) -> i32 {
    look::text_size(f, &text_of(h), None, false).0
}

/// A box `h` high, centred on the line whose middle is `mid`.
fn centred(x: i32, mid: i32, w: i32, h: i32) -> RECT {
    rect(x, mid - h / 2, w.max(0), h)
}

fn place_header(b: &mut Batch, c: &HeaderCtl, band: RECT) {
    let p = look::px;
    let mid = (band.top + band.bottom) / 2;
    b.place(
        c.chevron,
        centred(band.left + p(8.0), mid, p(22.0), p(22.0)),
    );
    let (sw, sh) = look::icon_size(Icon::Settings, true);
    let settings = centred(band.right - p(10.0) - sw, mid, sw, sh);
    b.place(c.settings, settings);
    let nw = text_w(c.new_worktree, Font::Small) + p(22.0);
    let new_wt = centred(settings.left - p(8.0) - nw, mid, nw, p(23.0));
    b.place(c.new_worktree, new_wt);
    let left = band.left + p(36.0);
    let limit = new_wt.left - p(12.0);

    let line1 = band.top + p(17.0);
    let name_w = text_w(c.name, Font::Title).min(limit - left);
    let title_h = look::line_height(Font::Title);
    b.place(c.name, centred(left, line1, name_w, title_h));
    let mut x = left + name_w + p(8.0);
    let meta_w = text_w(c.meta, Font::Caption).min(limit - x);
    b.place(
        c.meta,
        centred(x, line1 + p(1.0), meta_w, look::line_height(Font::Caption)),
    );
    x += meta_w.max(0) + p(8.0);
    b.place(c.spinner, centred(x, line1, p(14.0), p(14.0)));

    let line2 = band.top + p(38.0);
    let (cw, ch) = look::icon_size(Icon::Copy, true);
    let error = text_of(c.error);
    let error_w = if error.is_empty() {
        0
    } else {
        text_w(c.error, Font::Caption) + p(6.0)
    };
    // The path gives way before the error does.
    let room = limit - left - cw - p(4.0);
    let path_w = text_w(c.path, Font::Path)
        .min((room - error_w).max(p(80.0)))
        .min(room);
    b.place(
        c.path,
        centred(left, line2, path_w, look::line_height(Font::Path)),
    );
    let copy = centred(left + path_w + p(4.0), line2, cw, ch);
    b.place(c.copy, copy);
    if error.is_empty() {
        b.hide(c.error);
    } else {
        let ex = copy.right + p(6.0);
        b.place(
            c.error,
            centred(ex, line2, limit - ex, look::line_height(Font::Caption)),
        );
    }
}

fn place_worktree(b: &mut Batch, c: &WorktreeCtl, plate: RECT) {
    let p = look::px;
    let pad = p(12.0);
    let left = plate.left + pad;
    let right = plate.right - pad;

    // First line: branch and copy at the start; spinner, busy note and
    // badges at the end.
    let line1 = plate.top + p(18.0);
    let mut x = right;
    for h in c.badges.iter().rev() {
        let w = text_w(*h, Font::Badge) + p(14.0);
        x -= w;
        b.place(*h, centred(x, line1, w, p(18.0)));
        x -= p(4.0);
    }
    let busy = !text_of(c.busy).is_empty();
    if busy {
        let w = text_w(c.busy, Font::Caption);
        x -= p(2.0) + w;
        b.place(
            c.busy,
            centred(x, line1, w, look::line_height(Font::Caption)),
        );
        x -= p(6.0) + p(14.0);
        b.place(c.spinner, centred(x, line1, p(14.0), p(14.0)));
    } else {
        b.hide(c.busy);
        b.hide(c.spinner);
    }
    let limit = x - p(8.0);
    let copy_visible = c.last.as_ref().is_some_and(|l| l.on_copy_branch.is_some());
    let (cw, ch) = look::icon_size(Icon::Copy, true);
    let copy_room = if copy_visible { cw + p(4.0) } else { 0 };
    let natural = look::paint_of(key(c.pill))
        .and_then(|pt| match pt {
            Paint::Pill { rich, .. } => {
                Some(look::rich_size(&rich, wtm_toolkit::TextStyle::Branch).0)
            }
            _ => None,
        })
        .unwrap_or(0)
        + look::pill_extra();
    let pill_w = natural.min(limit - left - copy_room).max(p(60.0));
    b.place(c.pill, centred(left, line1, pill_w, p(24.0)));
    if copy_visible {
        b.place(
            c.copy_branch,
            centred(left + pill_w + p(4.0), line1, cw, ch),
        );
    } else {
        b.hide(c.copy_branch);
    }

    // Second line: path and copy at the start, the action groups at the
    // end with a wider gap between groups.
    let line2 = plate.top + p(42.0);
    let mut x = right;
    // Hidden actions are skipped; the gap is wider before the first of a
    // group.
    for a in c.actions.iter().rev() {
        if a.hidden {
            b.hide(a.hwnd);
            continue;
        }
        let (w, hh) = look::icon_size(a.icon, true);
        x -= w;
        b.place(a.hwnd, centred(x, line2, w, hh));
        x -= if a.group_start { p(12.0) } else { p(2.0) };
    }
    let limit = x - p(8.0);
    let path_w = text_w(c.path, Font::Path)
        .min(limit - left - cw - p(2.0))
        .max(0);
    b.place(
        c.path,
        centred(left, line2, path_w, look::line_height(Font::Path)),
    );
    b.place(c.copy_path, centred(left + path_w + p(2.0), line2, cw, ch));
}

fn place_pending(b: &mut Batch, c: &PendingCtl, plate: RECT) {
    let p = look::px;
    let pad = p(12.0);
    let mid = (plate.top + plate.bottom) / 2;
    let left = plate.left + pad;
    let right = plate.right - pad;
    let failed = c.last.as_ref().is_some_and(|l| l.error.is_some());
    let (dw, dh) = look::icon_size(Icon::Close, true);
    let mut limit = right;
    if failed {
        b.place(c.dismiss, centred(right - dw, mid, dw, dh));
        limit = right - dw - p(8.0);
    } else {
        b.hide(c.dismiss);
    }
    let bw = text_w(c.branch, Font::BranchBold).min(limit - left);
    b.place(
        c.branch,
        centred(left, mid, bw, look::line_height(Font::BranchBold)),
    );
    let mut x = left + bw + p(8.0);
    if failed {
        b.hide(c.spinner);
    } else {
        b.place(c.spinner, centred(x, mid, p(14.0), p(14.0)));
        x += p(14.0) + p(6.0);
    }
    b.place(
        c.status,
        centred(x, mid, limit - x, look::line_height(Font::Caption)),
    );
}

// MARK: Painting

fn paint_content(h: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(h, &mut ps) };
    let area = ps.rcPaint;
    let (w, hgt) = (area.right - area.left, area.bottom - area.top);
    if w > 0 && hgt > 0 {
        unsafe {
            // Drawn off screen and copied in, so a repaint never flickers.
            let mem = CreateCompatibleDC(Some(dc));
            let bmp = CreateCompatibleBitmap(dc, w, hgt);
            let old = SelectObject(mem, bmp.into());
            let _ = SetViewportOrgEx(mem, -area.left, -area.top, None);
            draw_cards(mem, area);
            let _ = SetViewportOrgEx(mem, 0, 0, None);
            let _ = BitBlt(dc, area.left, area.top, w, hgt, Some(mem), 0, 0, SRCCOPY);
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
        }
    }
    unsafe {
        let _ = EndPaint(h, &ps);
    }
}

fn draw_cards(dc: HDC, area: RECT) {
    let pal = look::palette();
    let s = look::scale() as f32;
    look::fill(dc, &area, pal.list);
    let (cards, selected, focused, drop_line, dragging) = look::with(|l| {
        (
            l.list
                .cards
                .iter()
                .map(|c| (c.key.clone(), c.rect, c.band_bottom, c.plates.clone()))
                .collect::<Vec<_>>(),
            l.list.selected.clone(),
            l.list.focused,
            l.list.drop_line,
            l.list.dragging.clone(),
        )
    });
    let Some(g) = look::Gfx::new(dc) else { return };
    let r = CARD_RADIUS as f32 * s;
    let pr = PLATE_RADIUS as f32 * s;
    let accent = if focused {
        pal.accent
    } else {
        blend(pal.accent, pal.secondary, 0.5)
    };
    for (k, c, band_bottom, plates) in &cards {
        if c.bottom < area.top || c.top > area.bottom {
            continue;
        }
        let (x, y) = (c.left as f32, c.top as f32);
        let (w, h) = ((c.right - c.left) as f32, (c.bottom - c.top) as f32);
        // A soft shadow: a few widening, fading outlines below the card.
        for i in 1..=3 {
            let d = i as f32 * s;
            g.fill_round(
                x - d / 2.0,
                y + d,
                w + d,
                h,
                [r + d; 4],
                argb(pal.text, 0.035),
            );
        }
        let open = !plates.is_empty();
        g.fill_round(
            x,
            y,
            w,
            h,
            [r; 4],
            argb(if open { pal.well } else { pal.band }, 1.0),
        );
        if open {
            let bh = (*band_bottom - c.top) as f32;
            g.fill_round(x, y, w, bh, [r, r, 0.0, 0.0], argb(pal.band, 1.0));
            g.lines(
                &[((x, y + bh), (x + w, y + bh))],
                1.0,
                argb(pal.card_border, 0.6),
            );
        }
        // A bright hairline along the top: the bevel the macOS card has.
        g.lines(
            &[((x + r, y + 1.5), (x + w - r, y + 1.5))],
            1.0,
            argb(pal.window, 0.7),
        );
        for (pk, p) in plates {
            let (px_, py) = (p.left as f32, p.top as f32);
            let (pw, ph) = ((p.right - p.left) as f32, (p.bottom - p.top) as f32);
            g.fill_round(px_, py + s, pw, ph, [pr; 4], argb(pal.text, 0.06));
            g.fill_round(px_, py, pw, ph, [pr; 4], argb(pal.plate, 1.0));
            g.stroke_round(
                px_ + 0.5,
                py + 0.5,
                pw - 1.0,
                ph - 1.0,
                [pr; 4],
                1.0,
                argb(pal.plate_border, 1.0),
            );
            if selected.as_deref() == Some(pk.as_str()) {
                g.stroke_round(
                    px_ + 1.0,
                    py + 1.0,
                    pw - 2.0,
                    ph - 2.0,
                    [pr; 4],
                    2.0 * s,
                    argb(accent, 0.9),
                );
            }
        }
        g.stroke_round(
            x + 0.5,
            y + 0.5,
            w - 1.0,
            h - 1.0,
            [r; 4],
            1.0,
            argb(pal.card_border, 1.0),
        );
        if selected.as_deref() == Some(k.as_str()) {
            let bh = (*band_bottom - c.top) as f32;
            let corners = if open { [r, r, 0.0, 0.0] } else { [r; 4] };
            g.stroke_round(
                x + 1.5,
                y + 1.5,
                w - 3.0,
                bh - 3.0,
                corners,
                2.0 * s,
                argb(accent, 0.9),
            );
        }
        if dragging.as_deref() == Some(k.as_str()) {
            // The card being dragged is shown lifted out: washed over.
            g.fill_round(x, y, w, h, [r; 4], argb(pal.window, 0.45));
        }
    }
    if let Some(line) = drop_line {
        let l = area.left.min(look::px(CARD_MARGIN)) as f32;
        let right =
            look::with(|lk| lk.list.cards.first().map(|c| c.rect.right)).unwrap_or(0) as f32;
        let y = line as f32;
        g.lines(
            &[((l + 4.0 * s, y), (right - 4.0 * s, y))],
            2.0 * s,
            argb(pal.accent, 1.0),
        );
        g.fill_round(
            l,
            y - 3.0 * s,
            6.0 * s,
            6.0 * s,
            [3.0 * s; 4],
            argb(pal.accent, 1.0),
        );
    }
}

/// What each row is called for assistive technology: what a sighted user
/// reads off it, in the order they would.
fn access_names(tree: &TreeList) -> HashMap<Key, String> {
    let mut out = HashMap::new();
    for s in &tree.sections {
        let h = &s.header;
        let mut name = format!("{}, {}", h.name, h.meta);
        if let Some(e) = &h.error {
            name.push_str(", ");
            name.push_str(e);
        }
        out.insert(s.key.clone(), name);
        for r in &s.rows {
            let name = match &r.content {
                RowContent::Worktree(w) => {
                    let mut parts = vec![w.branch_name.clone()];
                    // A badge that is only a symbol (the clean tick) is read
                    // by what its tooltip says.
                    parts.extend(w.badges.iter().map(|b| {
                        if b.text.chars().any(char::is_alphanumeric) {
                            b.text.clone()
                        } else {
                            b.tooltip.clone()
                        }
                    }));
                    parts.extend(w.busy.clone());
                    parts.join(", ")
                }
                RowContent::Pending(p) => match &p.error {
                    Some(e) => format!("{}, {e}", p.branch),
                    None => format!("{}, creating", p.branch),
                },
            };
            out.insert(r.key.clone(), name);
        }
    }
    out
}

// MARK: Window procedures

pub unsafe extern "system" fn frame_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_GETDLGCODE => LRESULT((DLGC_WANTARROWS | DLGC_WANTCHARS) as isize),
        WM_SETFOCUS | WM_KILLFOCUS => {
            let focused = msg == WM_SETFOCUS;
            look::with(|lk| lk.list.focused = focused);
            if let Ok(c) = unsafe { GetWindow(h, GW_CHILD) } {
                invalidate(c);
            }
            if focused {
                // The keyboard is on the list: a screen reader reads the
                // selected row, not just "Repositories".
                access::announce(true);
            }
            LRESULT(0)
        }
        WM_GETOBJECT => match access::get_object(h, w, l) {
            Some(r) => r,
            None => unsafe { DefWindowProcW(h, msg, w, l) },
        },
        WM_KEYDOWN => {
            let vk = VIRTUAL_KEY(w.0 as u16);
            let handled = with_state(|s| s.list_key(vk)).unwrap_or(false);
            if handled {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(h, msg, w, l) }
            }
        }
        WM_CHAR => LRESULT(0),
        WM_VSCROLL => {
            with_state(|s| s.list.vscroll(loword(w.0)));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(w.0) as i16;
            with_state(|s| s.list.wheel(delta));
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            look::fill(HDC(w.0 as *mut _), &client(h), look::palette().list);
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}

pub unsafe extern "system" fn content_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = container_msg(h, msg, w, l) {
        return r;
    }
    match msg {
        WM_PAINT => {
            paint_content(h);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONDOWN => {
            let (x, y) = point_of(l);
            with_state(|s| s.list_mouse(MouseEv::Down, x, y));
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = point_of(l);
            with_state(|s| s.list_mouse(MouseEv::Double, x, y));
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if w.0 & MK_LBUTTON.0 as usize != 0 {
                let (x, y) = point_of(l);
                with_state(|s| s.list_mouse(MouseEv::Move, x, y));
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point_of(l);
            with_state(|s| s.list_mouse(MouseEv::Up, x, y));
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with_state(|s| s.list_mouse(MouseEv::Lost, 0, 0));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MouseEv {
    Down,
    Double,
    Move,
    Up,
    Lost,
}
