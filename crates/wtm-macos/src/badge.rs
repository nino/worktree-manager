//! A small rounded status badge ("staged", "↑3 origin/main"), drawn natively:
//! a tinted capsule behind a system-font label. Colours are the system accent
//! set so they adapt to light and dark appearance and increased contrast.

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBezierPath, NSColor, NSFont, NSLayoutConstraint, NSTextField, NSView};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};
use std::cell::RefCell;

use crate::util::ns;

/// Which colour family a badge uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Clean,
    Staged,
    Unstaged,
    Untracked,
    Ahead,
    Behind,
    Unpushed,
    Missing,
    Muted,
    Primary,
}

impl BadgeTone {
    fn color(self) -> Retained<NSColor> {
        match self {
            BadgeTone::Clean => NSColor::systemGreenColor(),
            BadgeTone::Staged => NSColor::systemBlueColor(),
            BadgeTone::Unstaged => NSColor::systemOrangeColor(),
            BadgeTone::Untracked => NSColor::systemPurpleColor(),
            BadgeTone::Ahead => NSColor::systemTealColor(),
            BadgeTone::Behind => NSColor::systemRedColor(),
            BadgeTone::Unpushed => NSColor::systemYellowColor(),
            BadgeTone::Missing => NSColor::systemRedColor(),
            BadgeTone::Muted => NSColor::systemGrayColor(),
            BadgeTone::Primary => NSColor::controlAccentColor(),
        }
    }
}

pub struct BadgeIvars {
    label: Retained<NSTextField>,
    tone: RefCell<BadgeTone>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMBadge"]
    #[ivars = BadgeIvars]
    pub struct Badge;

    impl Badge {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let r = bounds.size.height / 2.0;
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, r, r);
            self.ivars().tone.borrow().color().colorWithAlphaComponent(0.16).setFill();
            path.fill();
        }
    }
);

impl Badge {
    pub fn new(
        text: &str,
        tone: BadgeTone,
        tooltip: &str,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let label = NSTextField::labelWithString(&ns(text), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize_weight(
            10.5,
            crate::util::MEDIUM,
        )));
        label.setTextColor(Some(&tone.color()));
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        let this = mtm.alloc::<Self>().set_ivars(BadgeIvars {
            label: label.clone(),
            tone: RefCell::new(tone),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(40.0, 16.0))]
        };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&label);
        this.setToolTip(Some(&ns(tooltip)));
        let constraints = [
            label
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&this.leadingAnchor(), 6.0),
            label
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&this.trailingAnchor(), -6.0),
            label
                .topAnchor()
                .constraintEqualToAnchor_constant(&this.topAnchor(), 1.5),
            label
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&this.bottomAnchor(), -1.5),
        ];
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        this
    }

    /// Re-style in place (cheaper than replacing the view).
    pub fn set(&self, text: &str, tone: BadgeTone, tooltip: &str) {
        let iv = self.ivars();
        iv.label.setStringValue(&ns(text));
        iv.label.setTextColor(Some(&tone.color()));
        *iv.tone.borrow_mut() = tone;
        self.setToolTip(Some(&ns(tooltip)));
        self.setNeedsDisplay(true);
    }
}

/// The badge row for a worktree, in display order: `(text, tone, tooltip)`.
pub fn badges_for(
    w: &wtm_core::WorktreeInfo,
    main_branch: &str,
) -> Vec<(String, BadgeTone, String)> {
    let mut out = Vec::new();
    if w.is_main {
        out.push((
            "primary".into(),
            BadgeTone::Primary,
            "The repository's primary working tree".into(),
        ));
    }
    if w.locked {
        out.push((
            "locked".into(),
            BadgeTone::Muted,
            "This worktree is locked".into(),
        ));
    }
    if w.prunable {
        out.push((
            "folder missing".into(),
            BadgeTone::Missing,
            "Folder was deleted outside the app — git still tracks this worktree. Delete the row to clean up git's bookkeeping.".into(),
        ));
        return out;
    }
    let Some(s) = &w.status else {
        out.push((
            "no status".into(),
            BadgeTone::Muted,
            "Status could not be determined".into(),
        ));
        return out;
    };
    let trunk = if s.trunk_ref.is_empty() {
        main_branch
    } else {
        &s.trunk_ref
    };
    if !s.is_dirty() {
        out.push((
            "✓".into(),
            BadgeTone::Clean,
            "Clean working tree — no staged, unstaged, or untracked changes".into(),
        ));
    }
    if s.has_staged {
        out.push((
            "staged".into(),
            BadgeTone::Staged,
            "Staged, uncommitted changes".into(),
        ));
    }
    if s.has_unstaged {
        out.push((
            "unstaged".into(),
            BadgeTone::Unstaged,
            "Unstaged changes".into(),
        ));
    }
    if s.has_untracked {
        out.push((
            "untracked".into(),
            BadgeTone::Untracked,
            "Untracked files".into(),
        ));
    }
    match (s.ahead_of_main, s.behind_main) {
        (Some(a), Some(b)) => {
            if a > 0 {
                out.push((
                    format!("↑{a} {trunk}"),
                    BadgeTone::Ahead,
                    format!("{a} commit(s) ahead of {trunk}"),
                ));
            }
            if b > 0 {
                out.push((
                    format!("↓{b} {trunk}"),
                    BadgeTone::Behind,
                    format!("{b} commit(s) behind {trunk}"),
                ));
            }
        }
        _ => out.push((
            format!("? {trunk}"),
            BadgeTone::Muted,
            format!("Couldn't compare with {trunk} — check the repo's main-branch setting"),
        )),
    }
    if s.unpushed {
        out.push((
            format!("⇡{} unpushed", s.unpushed_count),
            BadgeTone::Unpushed,
            format!("{} unpushed commit(s)", s.unpushed_count),
        ));
    } else if !s.has_upstream {
        out.push((
            "no upstream".into(),
            BadgeTone::Muted,
            "No upstream branch configured".into(),
        ));
    }
    out
}
