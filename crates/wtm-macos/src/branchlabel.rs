//! Rich text as it appears in the UI: `wtm_ui` swaps a `claude/` or
//! `cursor/` prefix for that agent's mark, drawn here by `toolicon`, so what
//! identifies the branch is not pushed off the end of a narrow row.

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::{
    NSAttributedStringAttachmentConveniences, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName,
};
use objc2_foundation::{
    NSAttributedString, NSCopying, NSDictionary, NSMutableAttributedString, NSString,
};

use crate::toolicon;
use crate::util::ns;

use wtm_core::branch_tool::BranchTool;
use wtm_toolkit::marks::{MARK_GAP, MARK_SIZE as MARK};
use wtm_toolkit::{Mark, Rich, Span};

/// `rich` as attributed text: each mark as the agent's mark, emphasised runs
/// in `bold` (the picker's fuzzy matches), the rest in `font`.
pub fn rich_label(
    rich: &Rich,
    font: &NSFont,
    ink: &NSColor,
    bold: Option<&NSFont>,
) -> Retained<NSAttributedString> {
    let text = NSMutableAttributedString::new();
    for span in &rich.spans {
        match span {
            Span::Mark(mark) => {
                let tool = match mark {
                    Mark::Claude => BranchTool::Claude,
                    Mark::Cursor => BranchTool::Cursor,
                };
                let image = toolicon::mark(tool, MARK, MARK_GAP, ink);
                let attachment = objc2_app_kit::NSTextAttachment::new();
                attachment.setImage(Some(&image));
                // Centre the mark on the text's cap band rather than its
                // baseline.
                let lift = (font.capHeight() - MARK) / 2.0;
                attachment.setBounds(objc2_foundation::NSRect::new(
                    objc2_foundation::NSPoint::new(0.0, lift),
                    objc2_foundation::NSSize::new(MARK + MARK_GAP, MARK),
                ));
                let mark = NSAttributedString::attributedStringWithAttachment(&attachment);
                text.appendAttributedString(&mark);
            }
            Span::Text { text: run, strong } => {
                let face = match (strong, bold) {
                    (true, Some(b)) => b,
                    _ => font,
                };
                let part = unsafe {
                    NSAttributedString::initWithString_attributes(
                        NSAttributedString::alloc(),
                        &ns(run),
                        Some(&attributes(face, ink)),
                    )
                };
                text.appendAttributedString(&part);
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
