//! [`Rich`] text as widgets: a row of agent marks (drawn) and labels, the
//! emphasised runs in bold. Patched in place: new text goes into the labels
//! that are there, and the children are rebuilt only when the marks move.

use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use wtm_toolkit::{Mark, Rich, Span};

use crate::draw;

/// How the labels of a [`RichLabel`] lay out their text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelOpts {
    /// Gives way with an ellipsis when there is not room.
    pub ellipsize: bool,
    pub wrap: bool,
    /// At most this many lines (with `wrap`); 0 for no limit.
    pub lines: i32,
    pub selectable: bool,
    pub xalign: f32,
}

impl Default for LabelOpts {
    fn default() -> Self {
        LabelOpts {
            ellipsize: false,
            wrap: false,
            lines: 0,
            selectable: false,
            xalign: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Mark(Mark),
    /// Pango markup of a run of text spans.
    Text(String),
}

fn pieces(rich: &Rich) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for span in &rich.spans {
        match span {
            Span::Mark(m) => out.push(Piece::Mark(*m)),
            Span::Text { text, strong } => {
                let escaped = glib::markup_escape_text(text);
                let run = if *strong {
                    format!("<b>{escaped}</b>")
                } else {
                    escaped.to_string()
                };
                match out.last_mut() {
                    Some(Piece::Text(t)) => t.push_str(&run),
                    _ => out.push(Piece::Text(run)),
                }
            }
        }
    }
    if out.is_empty() {
        out.push(Piece::Text(String::new()));
    }
    out
}

pub struct RichLabel {
    pub root: gtk::Box,
    pieces: RefCell<Vec<Piece>>,
    labels: RefCell<Vec<gtk::Label>>,
    opts: Cell<LabelOpts>,
}

impl RichLabel {
    pub fn new(rich: &Rich, opts: LabelOpts) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let this = RichLabel {
            root,
            pieces: RefCell::new(Vec::new()),
            labels: RefCell::new(Vec::new()),
            opts: Cell::new(opts),
        };
        this.set(rich);
        this
    }

    pub fn set(&self, rich: &Rich) {
        let new = pieces(rich);
        let same_shape = {
            let old = self.pieces.borrow();
            old.len() == new.len()
                && old.iter().zip(&new).all(|(a, b)| match (a, b) {
                    (Piece::Mark(x), Piece::Mark(y)) => x == y,
                    (Piece::Text(_), Piece::Text(_)) => true,
                    _ => false,
                })
        };
        if same_shape {
            let labels = self.labels.borrow();
            let mut li = 0;
            for (old, piece) in self.pieces.borrow().iter().zip(&new) {
                if let Piece::Text(markup) = piece {
                    if old != piece {
                        labels[li].set_markup(markup);
                    }
                    li += 1;
                }
            }
        } else {
            while let Some(child) = self.root.first_child() {
                self.root.remove(&child);
            }
            let mut labels = Vec::new();
            for piece in &new {
                match piece {
                    Piece::Mark(m) => self.root.append(&draw::mark(*m)),
                    Piece::Text(markup) => {
                        let label = gtk::Label::new(None);
                        label.set_markup(markup);
                        self.root.append(&label);
                        labels.push(label);
                    }
                }
            }
            *self.labels.borrow_mut() = labels;
            self.apply_opts(self.opts.get(), true);
        }
        *self.pieces.borrow_mut() = new;
    }

    pub fn set_opts(&self, opts: LabelOpts) {
        if opts != self.opts.get() {
            self.opts.set(opts);
            self.apply_opts(opts, true);
        }
    }

    fn apply_opts(&self, o: LabelOpts, _all: bool) {
        let labels = self.labels.borrow();
        let n = labels.len();
        for (i, l) in labels.iter().enumerate() {
            l.set_xalign(o.xalign);
            l.set_selectable(o.selectable);
            l.set_wrap(o.wrap);
            if o.wrap {
                l.set_wrap_mode(pango::WrapMode::WordChar);
                l.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
            }
            l.set_lines(if o.wrap { o.lines } else { -1 });
            l.set_ellipsize(if o.ellipsize || (o.wrap && o.lines > 0) {
                pango::EllipsizeMode::End
            } else {
                pango::EllipsizeMode::None
            });
            // The last run takes what room there is, so the text lines up
            // with its alignment rather than with the marks.
            l.set_hexpand(i + 1 == n && (o.wrap || o.xalign > 0.0));
            if o.ellipsize {
                l.set_width_chars(4);
            }
        }
    }
}
