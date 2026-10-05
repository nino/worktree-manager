//! What is drawn rather than taken from the icon theme: the agent marks (from
//! `wtm_toolkit::marks`), the branch button's chevrons, and the merge, editor
//! and terminal glyphs, which the freedesktop set has no symbolic action icon
//! for (`utilities-terminal` and `text-editor` are application icons, and
//! not every theme ships them as symbolic). All of them are cairo paths
//! in the widget's own text colour where they are not fixed, so they follow
//! the theme, a disabled button and a selected row.

use gtk::prelude::*;
use gtk::{cairo, gdk};

use wtm_toolkit::marks::{self, Paint, Shape, MARK_GAP};
use wtm_toolkit::{Icon, Mark};

/// The freedesktop icon for each of the app's icons; `None` for the ones
/// drawn here.
pub fn icon_name(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Add => "list-add-symbolic",
        Icon::Refresh => "view-refresh-symbolic",
        Icon::Settings => "emblem-system-symbolic",
        Icon::Copy => "edit-copy-symbolic",
        Icon::Push => "go-up-symbolic",
        Icon::Pull => "go-down-symbolic",
        Icon::Folder => "folder-symbolic",
        Icon::Delete => "user-trash-symbolic",
        Icon::Close => "window-close-symbolic",
        Icon::Merge | Icon::Editor | Icon::Terminal | Icon::Chevrons => return None,
    })
}

/// An icon at `size` pixels: from the theme where it has one.
pub fn icon(icon: Icon, size: i32) -> gtk::Widget {
    match icon_name(icon) {
        Some(name) => {
            let image = gtk::Image::from_icon_name(name);
            image.set_pixel_size(size);
            image.upcast()
        }
        None if icon == Icon::Chevrons => chevrons(),
        None => drawn(size, size, move |cr, w, h, ink| {
            let scale = w.min(h) / 16.0;
            cr.save().ok();
            cr.scale(scale, scale);
            set_ink(cr, ink, 1.0);
            cr.set_line_width(1.5);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            match icon {
                Icon::Merge => merge(cr),
                Icon::Editor => editor(cr),
                _ => terminal(cr),
            }
            cr.stroke().ok();
            cr.restore().ok();
        }),
    }
}

/// A drawing area of a fixed size whose `paint` gets the widget's colour.
fn drawn(
    width: i32,
    height: i32,
    paint: impl Fn(&cairo::Context, f64, f64, gdk::RGBA) + 'static,
) -> gtk::Widget {
    let area = gtk::DrawingArea::new();
    area.set_content_width(width);
    area.set_content_height(height);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(move |area, cr, w, h| {
        paint(cr, w as f64, h as f64, area.color());
    });
    area.upcast()
}

fn set_ink(cr: &cairo::Context, ink: gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(
        ink.red() as f64,
        ink.green() as f64,
        ink.blue() as f64,
        ink.alpha() as f64 * alpha,
    );
}

/// SF Symbols' `arrow.triangle.merge`: two lines that meet and go on up as
/// an arrow. These glyphs are paths on a 16-unit grid, stroked by the caller.
fn merge(cr: &cairo::Context) {
    cr.move_to(8.0, 8.5);
    cr.line_to(8.0, 2.5);
    cr.move_to(5.0, 5.5);
    cr.line_to(8.0, 2.5);
    cr.line_to(11.0, 5.5);
    cr.move_to(4.0, 14.0);
    cr.line_to(4.0, 12.5);
    cr.curve_to(4.0, 10.5, 8.0, 10.5, 8.0, 8.5);
    cr.move_to(12.0, 14.0);
    cr.line_to(12.0, 12.5);
    cr.curve_to(12.0, 10.5, 8.0, 10.5, 8.0, 8.5);
}

/// `</>`, as SF Symbols' `chevron.left.forwardslash.chevron.right`.
fn editor(cr: &cairo::Context) {
    cr.move_to(5.0, 4.5);
    cr.line_to(1.5, 8.0);
    cr.line_to(5.0, 11.5);
    cr.move_to(11.0, 4.5);
    cr.line_to(14.5, 8.0);
    cr.line_to(11.0, 11.5);
    cr.move_to(9.3, 3.0);
    cr.line_to(6.7, 13.0);
}

/// A window with a prompt in it, as SF Symbols' `terminal`.
fn terminal(cr: &cairo::Context) {
    rounded_rect(cr, 1.5, 2.5, 13.0, 11.0, 2.5);
    cr.move_to(4.5, 6.0);
    cr.line_to(7.0, 8.0);
    cr.line_to(4.5, 10.0);
    cr.move_to(8.5, 10.5);
    cr.line_to(11.5, 10.5);
}

/// Up-and-down chevrons, after the branch name: "this opens a list".
pub fn chevrons() -> gtk::Widget {
    drawn(8, 12, |cr, w, h, ink| {
        let (cx, cy) = (w / 2.0, h / 2.0);
        set_ink(cr, ink, 0.75);
        cr.set_line_width(1.3);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        cr.move_to(cx - 2.6, cy - 1.6);
        cr.line_to(cx, cy - 4.2);
        cr.line_to(cx + 2.6, cy - 1.6);
        cr.move_to(cx - 2.6, cy + 1.6);
        cr.line_to(cx, cy + 4.2);
        cr.line_to(cx + 2.6, cy + 1.6);
        cr.stroke().ok();
    })
}

/// An agent's mark in place of its branch prefix, sized to the text beside
/// it (the shapes are for 12-point text, which the branch style is).
pub fn mark(mark: Mark) -> gtk::Widget {
    let size = marks::MARK_SIZE.round() as i32 + 1;
    let w = drawn(size, size, move |cr, w, h, ink| {
        let scale = w.min(h) / 16.0;
        cr.save().ok();
        cr.translate((w - 16.0 * scale) / 2.0, (h - 16.0 * scale) / 2.0);
        cr.scale(scale, scale);
        for shape in marks::shapes(mark) {
            draw_shape(cr, &shape, ink);
        }
        cr.restore().ok();
    });
    w.set_margin_end(MARK_GAP as i32);
    w
}

fn paint(cr: &cairo::Context, paint: Paint, ink: gdk::RGBA) {
    match paint {
        Paint::Fixed(c) => cr.set_source_rgb(c.0, c.1, c.2),
        Paint::Ink(alpha) => set_ink(cr, ink, alpha),
    }
}

fn draw_shape(cr: &cairo::Context, shape: &Shape, ink: gdk::RGBA) {
    match shape {
        Shape::RoundedRect {
            x,
            y,
            w,
            h,
            r,
            paint: p,
        } => {
            rounded_rect(cr, *x, *y, *w, *h, *r);
            paint(cr, *p, ink);
            cr.fill().ok();
        }
        Shape::Polygon { points, paint: p } => {
            let mut first = true;
            for (x, y) in points {
                if first {
                    cr.move_to(*x, *y);
                    first = false;
                } else {
                    cr.line_to(*x, *y);
                }
            }
            cr.close_path();
            paint(cr, *p, ink);
            cr.fill().ok();
        }
        Shape::Lines {
            segments,
            width,
            paint: p,
        } => {
            for ((x1, y1), (x2, y2)) in segments {
                cr.move_to(*x1, *y1);
                cr.line_to(*x2, *y2);
            }
            cr.set_line_width(*width);
            paint(cr, *p, ink);
            cr.stroke().ok();
        }
    }
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}
