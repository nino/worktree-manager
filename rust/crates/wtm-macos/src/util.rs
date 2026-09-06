//! Small AppKit conveniences shared by the UI modules.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSColor, NSFont, NSImage, NSTextField};
use objc2_foundation::NSString;

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
