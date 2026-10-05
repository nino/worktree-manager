//! Dev switches for pictures of the running app, on a display without a
//! window manager (`xvfb-run` with `GSK_RENDERER=cairo`):
//!
//! - `WTM_SCREENSHOT=<png>` writes the main window to a PNG once the first
//!   listing has settled (no card still loading). An open dialog or the
//!   branch picker is drawn over it where it would be; an open panel
//!   (Settings) is written on its own.
//! - `WTM_SCREENSHOT_STEPS=<step>,<step>…` first drives the UI through the
//!   view's own handlers, as a click would: `new-worktree`, `repo-settings`,
//!   `picker`, `settings`, `collapse:<card>`, `select:<row>`,
//!   `query:<text>`, `type:<text>` (into the field with the keyboard).
//! - `WTM_SCREENSHOT_QUIT=1` quits once it is written.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib, graphene};

use wtm_toolkit::{RowContent, Span, View};

use crate::backend::Inner;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Waiting,
    Driving,
    Done,
}

pub struct Shot {
    path: PathBuf,
    quit: bool,
    steps: RefCell<VecDeque<String>>,
    stage: Cell<Stage>,
}

impl Shot {
    pub fn from_env() -> Option<Shot> {
        let path = std::env::var_os("WTM_SCREENSHOT").filter(|p| !p.is_empty())?;
        let steps = std::env::var("WTM_SCREENSHOT_STEPS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        Some(Shot {
            path: PathBuf::from(path),
            quit: std::env::var("WTM_SCREENSHOT_QUIT").is_ok_and(|v| v == "1"),
            steps: RefCell::new(steps),
            stage: Cell::new(Stage::Waiting),
        })
    }

    pub(crate) fn after_render(&self, inner: &Inner) {
        if self.stage.get() != Stage::Waiting {
            return;
        }
        let settled = inner.last.borrow().as_ref().is_some_and(settled);
        if !settled {
            return;
        }
        self.stage.set(Stage::Driving);
        let me = inner.me.clone();
        // Long enough for the statuses that follow the listing and for the
        // cards' own animation.
        glib::timeout_add_local_once(Duration::from_millis(1200), move || {
            if let Some(i) = me.upgrade() {
                if let Some(s) = i.shot.as_ref() {
                    s.next(&i);
                }
            }
        });
    }

    fn next(&self, inner: &Inner) {
        let step = self.steps.borrow_mut().pop_front();
        let me = inner.me.clone();
        let again = move || {
            if let Some(i) = me.upgrade() {
                if let Some(s) = i.shot.as_ref() {
                    s.next(&i);
                }
            }
        };
        match step {
            Some(step) => {
                drive(inner, &step);
                glib::timeout_add_local_once(Duration::from_millis(900), again);
            }
            None => {
                // A card still opening or closing would be caught mid-way.
                let moving = inner
                    .main
                    .borrow()
                    .as_ref()
                    .is_some_and(|m| !m.list.settled());
                if moving {
                    glib::timeout_add_local_once(Duration::from_millis(100), again);
                    return;
                }
                self.stage.set(Stage::Done);
                match capture(inner, &self.path) {
                    Ok(()) => log::info!("screenshot written to {}", self.path.display()),
                    Err(e) => log::error!("screenshot failed: {e}"),
                }
                if self.quit {
                    inner.quit();
                }
            }
        }
    }
}

fn settled(v: &View) -> bool {
    v.window.empty.is_some()
        || (!v.window.list.sections.is_empty()
            && v.window.list.sections.iter().all(|s| !s.header.loading))
}

/// One step, through the handlers the last view gave its widgets.
fn drive(inner: &Inner, step: &str) {
    let Some(view) = inner.last.borrow().clone() else {
        return;
    };
    let (name, arg) = step.split_once(':').unwrap_or((step, ""));
    let index = arg.parse::<usize>().unwrap_or(0);
    let list = &view.window.list;
    match name {
        "new-worktree" => {
            if let Some(s) = list.sections.get(index) {
                s.header.on_new_worktree.call(());
            }
        }
        "repo-settings" => {
            if let Some(s) = list.sections.get(index) {
                s.header.on_settings.call(());
            }
        }
        "settings" => {
            if let Some(h) = view.toolbar_button("settings") {
                h.call(());
            }
        }
        "collapse" => {
            if let Some(s) = list.sections.get(index) {
                list.on_toggle.call((s.key.clone(), false));
            }
        }
        "select" => {
            if let Some(k) = list.visible_keys().get(index) {
                list.on_select.call(Some(k.to_string()));
            }
        }
        "query" => {
            if let Some((_, h)) = view.search() {
                h.call(arg.to_string());
            }
        }
        "type" => {
            let focus = inner
                .window
                .borrow()
                .iter()
                .chain(inner.dialogs.window().iter())
                .filter_map(gtk::prelude::RootExt::focus)
                .next_back();
            if let Some(e) = focus.and_then(|f| f.dynamic_cast::<gtk::Editable>().ok()) {
                e.set_text(arg);
                e.set_position(-1);
            }
        }
        "picker" => {
            // An agent's branch if there is one: the picker shows its mark.
            let rows: Vec<_> = list
                .sections
                .iter()
                .flat_map(|s| s.rows.iter())
                .filter_map(|r| match &r.content {
                    RowContent::Worktree(w) => Some(w),
                    RowContent::Pending(_) => None,
                })
                .collect();
            let pick = rows
                .iter()
                .find(|w| w.branch.spans.iter().any(|s| matches!(s, Span::Mark(_))))
                .or(rows.first());
            if let Some(w) = pick {
                w.on_switch.call(());
            }
        }
        other => log::warn!("unknown screenshot step {other:?}"),
    }
}

fn paintable_size(w: &impl IsA<gtk::Widget>) -> (f64, f64) {
    (w.width() as f64, w.height() as f64)
}

fn capture(inner: &Inner, path: &std::path::Path) -> Result<(), String> {
    let main = inner.window.borrow().clone().ok_or("no window")?;
    let renderer = main
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    let snap = gtk::Snapshot::new();
    let panel = inner.panels.any_window();
    let (width, height) = if let Some(p) = &panel {
        let (w, h) = paintable_size(p);
        gtk::WidgetPaintable::new(Some(p)).snapshot(&snap, w, h);
        (w, h)
    } else {
        let (w, h) = paintable_size(&main);
        gtk::WidgetPaintable::new(Some(&main)).snapshot(&snap, w, h);
        if let Some(d) = inner.dialogs.window() {
            // Dimmed behind, centred over it, as a modal dialog sits.
            snap.append_color(
                &gdk::RGBA::new(0.0, 0.0, 0.0, 0.25),
                &graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
            );
            let (dw, dh) = paintable_size(&d);
            snap.save();
            snap.translate(&graphene::Point::new(
                ((w - dw) / 2.0).max(0.0) as f32,
                ((h - dh) / 3.0).max(0.0) as f32,
            ));
            gtk::WidgetPaintable::new(Some(&d)).snapshot(&snap, dw, dh);
            snap.restore();
        }
        if let Some(p) = inner.picker.popover() {
            if let Some(anchor) = p.parent() {
                if let Some(b) = anchor.compute_bounds(&main) {
                    let (pw, ph) = paintable_size(&p);
                    let x = (b.x() + b.width() / 2.0) as f64 - pw / 2.0;
                    let y = (b.y() + b.height()) as f64;
                    snap.save();
                    snap.translate(&graphene::Point::new(x.max(0.0) as f32, y as f32));
                    gtk::WidgetPaintable::new(Some(&p)).snapshot(&snap, pw, ph);
                    snap.restore();
                }
            }
        }
        (w, h)
    };
    let node = snap.to_node().ok_or("nothing drawn")?;
    let clip = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&clip));
    texture.save_to_png(path).map_err(|e| e.to_string())
}
