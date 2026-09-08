//! Small AppKit conveniences shared by the UI modules.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSColor, NSFont, NSImage, NSImageSymbolConfiguration, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// `NSFontWeight` values (the AppKit constants are extern statics, which are
/// unsafe to read; the numbers are documented and stable).
pub const REGULAR: f64 = 0.0;
pub const MEDIUM: f64 = 0.23;
pub const SEMIBOLD: f64 = 0.3;

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
