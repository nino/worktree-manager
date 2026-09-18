//! Small AppKit conveniences shared by the UI modules.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSFont, NSImage,
    NSImageSymbolConfiguration, NSTextField,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

/// `NSFontWeight` values (the AppKit constants are extern statics, which are
/// unsafe to read; the numbers are documented and stable).
pub const REGULAR: f64 = 0.0;
pub const MEDIUM: f64 = 0.23;
pub const SEMIBOLD: f64 = 0.3;

/// Dark Aqua, from the appearance currently being drawn into. Lit edges,
/// grain and glows that read as bevels in light read as extra borders here.
pub fn drawing_dark() -> bool {
    let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe {
        NSAppearanceNameDarkAqua
    }]);
    NSAppearance::currentDrawingAppearance()
        .bestMatchFromAppearancesWithNames(&names)
        .is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua })
}

pub fn by_appearance(light: f64, dark: f64) -> f64 {
    if drawing_dark() {
        dark
    } else {
        light
    }
}

/// Branch-name ink: `labelColor`, blended toward the plate when dark so full
/// white doesn't bloom. The blend is a static colour — rebuild it under the
/// view's appearance when that changes (`performAsCurrentDrawingAppearance`).
pub fn primary_ink() -> Retained<NSColor> {
    let base = NSColor::labelColor();
    if !drawing_dark() {
        return base;
    }
    base.blendedColorWithFraction_ofColor(0.15, &NSColor::controlBackgroundColor())
        .unwrap_or(base)
}

/// Regular monospace for branch names. Semibold packed too much lit ink into
/// an unbroken run of glyphs; the bezel carries the emphasis instead.
pub fn branch_font() -> Retained<NSFont> {
    NSFont::monospacedSystemFontOfSize_weight(12.0, REGULAR)
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

/// An SF Symbol image, or `None` on a system without it.
pub fn symbol(name: &str, description: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), Some(&ns(description)))
}

/// An SF Symbol at `size`/`weight`, with `lead` points of space before it and
/// raised by `dy`: a button centres its image on the title's line box, which
/// includes descender space the text beside it may not use, leaving the symbol
/// looking low, and butts it right up against the title. Both are baked into
/// the image as padding, so no layout maths depends on them.
pub fn symbol_raised(
    name: &str,
    description: &str,
    size: f64,
    weight: f64,
    lead: f64,
    dy: f64,
) -> Option<Retained<NSImage>> {
    let base = symbol(name, description)?;
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(size, weight);
    let base = base.imageWithSymbolConfiguration(&config)?;
    let inner = base.size();
    let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
        base.drawInRect(NSRect::new(NSPoint::new(lead, 2.0 * dy), inner));
        Bool::YES
    });
    let padded = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(inner.width + lead, inner.height + 2.0 * dy),
        false,
        &handler,
    );
    padded.setTemplate(true);
    Some(padded)
}

/// A single-line, non-editable label.
pub fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&ns(text), mtm);
    l.setUsesSingleLineMode(true);
    l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingMiddle);
    l
}

pub fn secondary_label(text: &str, size: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    l
}

pub fn mono_label(text: &str, size: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    l.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
        size, REGULAR,
    )));
    l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    l
}
