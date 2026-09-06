//! Branch names as they appear in the UI: a `claude/` or `cursor/` prefix is
//! swapped for that agent's mark (see `toolicon`), so what identifies the
//! branch is not pushed off the end of a narrow row. Everything else — the
//! picker's fuzzy matching, the tooltip, the value dispatched on a switch —
//! keeps the whole name.

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::{
    NSAttributedStringAttachmentConveniences, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName,
};
use objc2_foundation::{
    NSAttributedString, NSCopying, NSDictionary, NSMutableAttributedString, NSRange, NSString,
};
use wtm_core::branch_tool::split_tool_prefix;

use crate::toolicon;
use crate::util::ns;

/// Mark size and the space after it, in points.
const MARK: f64 = 13.0;
const MARK_GAP: f64 = 3.0;

/// `branch`, with its agent prefix drawn as a mark. `emphasis` bolds the
/// characters at those indices into `branch` (the picker's fuzzy matches);
/// indices inside the prefix have nothing left to bold and are dropped.
pub fn branch_label(
    branch: &str,
    font: &NSFont,
    ink: &NSColor,
    emphasis: Option<(&NSFont, &[usize])>,
) -> Retained<NSAttributedString> {
    let split = split_tool_prefix(branch);
    let (prefix_chars, rest) = match split {
        Some((_, rest)) => (branch.chars().count() - rest.chars().count(), rest),
        None => (0, branch),
    };
    let attrs = attributes(font, ink);
    let text = unsafe {
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &ns(rest),
            Some(&attrs),
        )
    };
    if let Some((tool, _)) = split {
        let image = toolicon::mark(tool, MARK, MARK_GAP, ink);
        let attachment = objc2_app_kit::NSTextAttachment::new();
        attachment.setImage(Some(&image));
        // Centre the mark on the text's cap band rather than its baseline.
        let lift = (font.capHeight() - MARK) / 2.0;
        attachment.setBounds(objc2_foundation::NSRect::new(
            objc2_foundation::NSPoint::new(0.0, lift),
            objc2_foundation::NSSize::new(MARK + MARK_GAP, MARK),
        ));
        let mark = NSAttributedString::attributedStringWithAttachment(&attachment);
        text.insertAttributedString_atIndex(&mark, 0);
    }
    if let Some((bold, matched)) = emphasis {
        let bold_attrs = bold_attributes(bold);
        // One UTF-16 unit for the mark, then the rest of the name.
        let head = if split.is_some() { 1 } else { 0 };
        let offsets: Vec<usize> = rest
            .chars()
            .scan(head, |acc, c| {
                let here = *acc;
                *acc += c.len_utf16();
                Some(here)
            })
            .collect();
        for i in matched.iter().filter_map(|i| i.checked_sub(prefix_chars)) {
            if let (Some(&at), Some(c)) = (offsets.get(i), rest.chars().nth(i)) {
                unsafe { text.addAttributes_range(&bold_attrs, NSRange::new(at, c.len_utf16())) };
            }
        }
    }
    Retained::into_super(text)
}

fn attributes(font: &NSFont, ink: &NSColor) -> Retained<NSDictionary<NSString>> {
    unsafe {
        NSDictionary::from_retained_objects::<NSString>(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[
                Retained::into_super(Retained::into_super(font.copy())),
                Retained::into_super(Retained::into_super(ink.copy())),
            ],
        )
    }
}

fn bold_attributes(font: &NSFont) -> Retained<NSDictionary<NSString>> {
    unsafe {
        NSDictionary::from_retained_objects::<NSString>(
            &[NSFontAttributeName],
            &[Retained::into_super(Retained::into_super(font.copy()))],
        )
    }
}
