//! The look: one stylesheet for the cards, badges and buttons, the text
//! styles mapped to classes, and the appearance (light or dark).
//!
//! Colours are the theme's named colours or blends of them, so a theme or
//! the dark variant restyles everything. GTK's own theme (Adwaita as built
//! into GTK 4.14) predates libadwaita's names (`@accent_color`,
//! `@window_fg_color`, …), so they are defined here as fallbacks over the
//! older ones, at the lowest priority: a theme that has them wins.

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use wtm_toolkit::{Emphasis, Hue, Ink, TextStyle};

/// The class that turns on the dark rules below. Set on every toplevel.
pub const DARK: &str = "wtm-dark";

const FALLBACK_COLOURS: &str = r#"
@define-color accent_color @theme_selected_bg_color;
@define-color accent_bg_color @theme_selected_bg_color;
@define-color window_bg_color @theme_bg_color;
@define-color window_fg_color @theme_fg_color;
@define-color view_bg_color @theme_base_color;
@define-color view_fg_color @theme_text_color;
"#;

/// Badge hues. GTK names no system palette beyond accent, error, warning and
/// success, so the rest are the GNOME palette's darker steps, which stay
/// legible as text on their own pale fill.
const STYLES: &str = r#"
@define-color wtm_green #26a269;
@define-color wtm_blue #1c71d8;
@define-color wtm_orange mix(@warning_color, black, 0.08);
@define-color wtm_purple #813d9c;
@define-color wtm_teal #2190a4;
@define-color wtm_red @error_color;
@define-color wtm_yellow #9c6e03;
@define-color wtm_gray #77767b;
@define-color wtm_band_blue mix(@accent_color, #8e8e93, 0.45);

/* MARK: Text styles (macOS points, scaled to the theme's own size: 13 → 1em) */
.t-heading { font-size: 1.23em; font-weight: 600; }
.t-title { font-size: 1.08em; font-weight: 600; }
.t-small { font-size: 0.92em; }
.t-caption { font-size: 0.85em; }
.t-path { font-family: monospace; font-size: 0.81em; }
.t-branch { font-family: monospace; font-size: 0.92em; }
.t-branch-strong { font-family: monospace; font-size: 0.92em; font-weight: 600; }
.ink-secondary { opacity: 0.62; }
.ink-error { color: @error_color; }

/* MARK: The list: raised cards, a recessed well, plates on it */
.wtm-list { background-color: @window_bg_color; padding-bottom: 14px; }
.wtm-card {
  margin: 12px 14px 0 14px;
  border-radius: 10px;
  background-color: @view_bg_color;
  border: 1px solid alpha(@borders, 0.75);
  box-shadow: 0 1px 3px alpha(black, 0.14);
}
.wtm-header {
  padding: 8px 14px 7px 4px;
  background-image: linear-gradient(to bottom,
      mix(mix(@view_bg_color, @wtm_band_blue, 0.26), black, 0.06),
      mix(mix(@view_bg_color, @wtm_band_blue, 0.12), black, 0.02));
  box-shadow: inset 0 1px alpha(white, 0.35);
}
.wtm-dark .wtm-header {
  background-image: linear-gradient(to bottom,
      mix(mix(@view_bg_color, @wtm_band_blue, 0.05), black, 0.02),
      mix(mix(@view_bg_color, @wtm_band_blue, 0.12), black, 0.05));
  box-shadow: inset 0 1px alpha(white, 0.10);
}
.wtm-well {
  background-color: mix(@window_bg_color, black, 0.02);
  border-top: 1px solid alpha(@borders, 0.5);
  padding: 6px 10px 6px 10px;
  box-shadow: inset 0 4px 4px -3px alpha(black, 0.10),
              inset 5px 0 4px -4px alpha(black, 0.06),
              inset -5px 0 4px -4px alpha(black, 0.06);
}
.wtm-dark .wtm-well { background-color: mix(@window_bg_color, black, 0.42); }
.wtm-plate {
  margin: 4px 0;
  padding: 7px 10px 7px 12px;
  border-radius: 7px;
  background-color: @view_bg_color;
  border: 1px solid alpha(@borders, 0.6);
  box-shadow: 0 1px 4px alpha(black, 0.16), inset 0 1px alpha(white, 0.30);
}
.wtm-dark .wtm-plate { box-shadow: 0 1px 4px alpha(black, 0.30), inset 0 1px alpha(white, 0.06); }
/* The selection is drawn as part of the card; the focus ring would double it. */
.wtm-header:focus-visible, .wtm-plate:focus-visible { outline: none; }
.wtm-header.selected {
  box-shadow: inset 0 0 0 2px alpha(@accent_color, 0.9), inset 0 1px alpha(white, 0.35);
}
.wtm-plate.selected {
  border-color: alpha(@accent_color, 0.9);
  box-shadow: 0 0 0 1px alpha(@accent_color, 0.9), 0 0 5px alpha(@accent_color, 0.7);
}
.wtm-dark .wtm-header.selected { box-shadow: inset 0 0 0 2px alpha(@accent_color, 0.75); }
.wtm-dark .wtm-plate.selected {
  border-color: alpha(@accent_color, 0.75);
  box-shadow: 0 0 0 1px alpha(@accent_color, 0.75), 0 0 3px alpha(@accent_color, 0.30);
}
.wtm-drop-before { box-shadow: 0 -3px 0 0 @accent_color; }

/* MARK: Badges: in light each hue tints its text on a pale fill */
.wtm-badge {
  border-radius: 999px;
  padding: 1px 6px;
  font-size: 0.81em;
  font-weight: 500;
}
.wtm-badge.hue-green { color: @wtm_green; background-color: alpha(@wtm_green, 0.16); }
.wtm-badge.hue-blue { color: @wtm_blue; background-color: alpha(@wtm_blue, 0.16); }
.wtm-badge.hue-orange { color: @wtm_orange; background-color: alpha(@wtm_orange, 0.16); }
.wtm-badge.hue-purple { color: @wtm_purple; background-color: alpha(@wtm_purple, 0.16); }
.wtm-badge.hue-teal { color: @wtm_teal; background-color: alpha(@wtm_teal, 0.16); }
.wtm-badge.hue-red { color: @wtm_red; background-color: alpha(@wtm_red, 0.16); }
.wtm-badge.hue-yellow { color: @wtm_yellow; background-color: alpha(@wtm_yellow, 0.16); }
.wtm-badge.hue-gray { color: @wtm_gray; background-color: alpha(@wtm_gray, 0.16); }
.wtm-badge.hue-accent { color: @accent_color; background-color: alpha(@accent_color, 0.16); }
/* Dark rations colour: against near-black a stack of rows turns hues into a
   repeating pattern, so only uncommitted work and a missing folder get any. */
.wtm-dark .wtm-badge.emph-quiet {
  color: alpha(@window_fg_color, 0.62); background-color: alpha(@window_fg_color, 0.06);
}
.wtm-dark .wtm-badge.emph-notable {
  color: @window_fg_color; background-color: alpha(@window_fg_color, 0.10);
}
.wtm-dark .wtm-badge.emph-attention {
  color: mix(@warning_color, @window_fg_color, 0.3);
  background-color: alpha(mix(@warning_color, @window_fg_color, 0.3), 0.14);
}
.wtm-dark .wtm-badge.emph-alarm {
  color: mix(@error_color, @window_fg_color, 0.3);
  background-color: alpha(mix(@error_color, @window_fg_color, 0.3), 0.14);
}

/* MARK: Buttons */
button.wtm-icon {
  min-width: 0; min-height: 0;
  padding: 3px;
  border-radius: 5px;
  color: alpha(@window_fg_color, 0.62);
}
button.wtm-icon:hover { color: @window_fg_color; }
/* Red in light; dark gets the other icons' grey: a red trash on every row is
   a column of the loudest colour, and delete already confirms. */
button.wtm-icon.danger { color: @error_color; }
.wtm-dark button.wtm-icon.danger { color: alpha(@window_fg_color, 0.62); }
button.wtm-small { min-height: 0; padding: 1px 8px; font-size: 0.85em; }
button.wtm-accessory { min-height: 0; padding: 1px 6px; font-size: 0.85em; }
button.wtm-pill {
  min-height: 0;
  padding: 1px 6px 1px 6px;
  border-radius: 6px;
  background-image: none;
  background-color: alpha(@window_fg_color, 0.05);
  border: 1px solid alpha(@borders, 0.6);
  box-shadow: none;
}
button.wtm-pill:hover { background-color: alpha(@window_fg_color, 0.10); }
button.wtm-disclosure { min-width: 0; min-height: 0; padding: 2px; color: alpha(@window_fg_color, 0.62); }

/* MARK: Window furniture */
.wtm-notice {
  padding: 6px 10px 6px 14px;
  background-color: mix(@window_bg_color, @accent_color, 0.08);
  border-bottom: 1px solid alpha(@borders, 0.6);
}
.wtm-empty { padding: 40px; }
.wtm-activity { margin: 10px; }
.wtm-dialog { padding: 20px 22px 18px 22px; }
.wtm-dialog .wtm-dialog-title { font-weight: 700; font-size: 1.08em; }
.wtm-dialog-icon.warning { color: @warning_color; }
.wtm-dialog-icon.critical { color: @error_color; }
.wtm-panel { padding: 20px 22px; }
.wtm-textblock { font-family: monospace; font-size: 0.85em; }
.wtm-picker list { background: none; }
.wtm-picker row { padding: 3px 6px; border-radius: 5px; }
"#;

/// Install the stylesheets on the default display. Once per process.
pub fn install() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let fallback = gtk::CssProvider::new();
    fallback.load_from_string(FALLBACK_COLOURS);
    gtk::style_context_add_provider_for_display(
        &display,
        &fallback,
        gtk::STYLE_PROVIDER_PRIORITY_FALLBACK,
    );
    let styles = gtk::CssProvider::new();
    styles.load_from_string(STYLES);
    gtk::style_context_add_provider_for_display(
        &display,
        &styles,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

// MARK: Classes

pub fn text_class(style: TextStyle) -> Option<&'static str> {
    Some(match style {
        TextStyle::Body => return None,
        TextStyle::Heading => "t-heading",
        TextStyle::Title => "t-title",
        TextStyle::Small => "t-small",
        TextStyle::Caption => "t-caption",
        TextStyle::Path => "t-path",
        TextStyle::Branch => "t-branch",
        TextStyle::BranchStrong => "t-branch-strong",
    })
}

pub fn ink_class(ink: Ink) -> Option<&'static str> {
    match ink {
        Ink::Primary => None,
        Ink::Secondary => Some("ink-secondary"),
        Ink::Error => Some("ink-error"),
    }
}

pub fn hue_class(hue: Hue) -> &'static str {
    match hue {
        Hue::Green => "hue-green",
        Hue::Blue => "hue-blue",
        Hue::Orange => "hue-orange",
        Hue::Purple => "hue-purple",
        Hue::Teal => "hue-teal",
        Hue::Red => "hue-red",
        Hue::Yellow => "hue-yellow",
        Hue::Gray => "hue-gray",
        Hue::Accent => "hue-accent",
    }
}

pub fn emphasis_class(e: Emphasis) -> &'static str {
    match e {
        Emphasis::Quiet => "emph-quiet",
        Emphasis::Notable => "emph-notable",
        Emphasis::Attention => "emph-attention",
        Emphasis::Alarm => "emph-alarm",
    }
}

/// Replace the classes a widget had for one property (`old`) with the ones
/// it has now, touching nothing when they are the same.
pub fn swap_classes(w: &impl IsA<gtk::Widget>, old: &[&str], new: &[&str]) {
    for c in old {
        if !new.contains(c) {
            w.remove_css_class(c);
        }
    }
    for c in new {
        if !old.contains(c) {
            w.add_css_class(c);
        }
    }
}

// MARK: Appearance

/// Apply `WTM_APPEARANCE=dark|light` if set, else follow the desktop's
/// preference as the settings portal reports it, now and as it changes:
/// GTK 4.14 reads only its own `prefer-dark` setting, and nothing sets that
/// from the desktop without libadwaita.
pub fn init_appearance() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    match std::env::var("WTM_APPEARANCE").ok().as_deref() {
        Some("dark") => settings.set_gtk_application_prefer_dark_theme(true),
        Some("light") => {
            settings.set_gtk_application_prefer_dark_theme(false);
            // A dark theme variant chosen by name is dark whatever the
            // preference says.
            let name = settings.gtk_theme_name().unwrap_or_default();
            if let Some(light) = name.strip_suffix("-dark") {
                settings.set_gtk_theme_name(Some(light));
            }
        }
        _ => follow_portal_scheme(settings.is_gtk_application_prefer_dark_theme()),
    }
    settings.connect_gtk_application_prefer_dark_theme_notify(|_| sync_dark());
    settings.connect_gtk_theme_name_notify(|_| sync_dark());
}

/// What `prefer-dark` should be for the portal's `color-scheme`: 1 prefers
/// dark, 2 light, and 0 (no preference) or anything else leaves GTK's own
/// setting, `own`, as it was before the portal was heard.
pub fn prefer_dark_for(scheme: u32, own: bool) -> bool {
    match scheme {
        1 => true,
        2 => false,
        _ => own,
    }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";

thread_local! {
    /// The portal subscription, kept for as long as the app runs.
    static SCHEME_CHANGES: std::cell::RefCell<Option<gio::SignalSubscription>> =
        const { std::cell::RefCell::new(None) };
}

/// `org.freedesktop.appearance color-scheme`, read once and then followed
/// through `SettingChanged`, so the app turns dark or light with the
/// desktop. Asked without waiting; no session bus or no portal (a bare X
/// server) leaves the theme as it is. `own` is GTK's setting beforehand.
fn follow_portal_scheme(own: bool) {
    let apply = move |v: &glib::Variant| {
        // The value comes wrapped in a variant, twice from `Read`.
        let mut v = v.clone();
        // Checked first: `as_variant` on anything else is a GLib critical.
        while v.is_type(glib::VariantTy::VARIANT) {
            match v.as_variant() {
                Some(inner) => v = inner,
                None => return,
            }
        }
        if let (Some(scheme), Some(settings)) = (v.get::<u32>(), gtk::Settings::default()) {
            let dark = prefer_dark_for(scheme, own);
            if settings.is_gtk_application_prefer_dark_theme() != dark {
                settings.set_gtk_application_prefer_dark_theme(dark);
            }
        }
    };
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |conn| {
        let Ok(conn) = conn else { return };
        // Subscribed before the read, so a change between the two is not
        // missed.
        let subscription = conn.subscribe_to_signal(
            Some(PORTAL),
            Some(PORTAL_SETTINGS),
            Some("SettingChanged"),
            Some(PORTAL_PATH),
            Some(APPEARANCE),
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let p = signal.parameters;
                if p.n_children() == 3 && p.child_value(1).str() == Some("color-scheme") {
                    apply(&p.child_value(2));
                }
            },
        );
        SCHEME_CHANGES.with(|s| *s.borrow_mut() = Some(subscription));
        conn.call(
            Some(PORTAL),
            PORTAL_PATH,
            PORTAL_SETTINGS,
            "Read",
            Some(&(APPEARANCE, "color-scheme").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
            move |reply| {
                if let Ok(reply) = reply {
                    apply(&reply.child_value(0));
                }
            },
        );
    });
}

/// Whether the theme is drawing dark now.
pub fn is_dark() -> bool {
    let Some(settings) = gtk::Settings::default() else {
        return false;
    };
    settings.is_gtk_application_prefer_dark_theme()
        || settings
            .gtk_theme_name()
            .is_some_and(|n| n.to_lowercase().ends_with("-dark"))
}

/// Put the dark class on every toplevel, or take it off.
pub fn sync_dark() {
    let dark = is_dark();
    let windows = gtk::Window::list_toplevels();
    for w in windows {
        mark_dark(&w, dark);
    }
}

pub fn mark_dark(w: &impl IsA<gtk::Widget>, dark: bool) {
    if dark {
        w.add_css_class(DARK);
    } else {
        w.remove_css_class(DARK);
    }
}

/// For a window made after the appearance was set.
pub fn adopt(w: &impl IsA<gtk::Widget>) {
    mark_dark(w, is_dark());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_scheme_overrides_gtk_only_when_it_has_a_preference() {
        assert!(prefer_dark_for(1, false));
        assert!(!prefer_dark_for(2, true));
        // No preference: GTK's own setting stands, whichever it was.
        assert!(prefer_dark_for(0, true));
        assert!(!prefer_dark_for(0, false));
        assert!(prefer_dark_for(7, true));
    }
}
