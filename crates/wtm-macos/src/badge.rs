//! A small rounded status badge ("staged", "↑3 origin/main"), drawn natively:
//! a tinted capsule behind a system-font label. Colour is reserved for
//! uncommitted work and a missing folder; everything else is luminance.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAppearanceCustomization, NSBezierPath, NSColor, NSFont, NSLayoutConstraint, NSTextField,
    NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};
use std::cell::RefCell;

use crate::util::ns;

/// What a badge is reporting. Loudness is [`BadgeRank`].
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

/// How loud a badge is drawn. Only Attention and Alarm spend colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeRank {
    Quiet,
    Notable,
    Attention,
    Alarm,
}

impl BadgeTone {
    pub fn rank(self) -> BadgeRank {
        match self {
            BadgeTone::Staged | BadgeTone::Unstaged | BadgeTone::Untracked => BadgeRank::Attention,
            BadgeTone::Missing => BadgeRank::Alarm,
            BadgeTone::Unpushed => BadgeRank::Notable,
            BadgeTone::Clean
            | BadgeTone::Ahead
            | BadgeTone::Behind
            | BadgeTone::Muted
            | BadgeTone::Primary => BadgeRank::Quiet,
        }
    }
}

impl BadgeRank {
    fn ink(self) -> Retained<NSColor> {
        match self {
            BadgeRank::Quiet => NSColor::secondaryLabelColor(),
            BadgeRank::Notable => NSColor::labelColor(),
            BadgeRank::Attention => muted(&NSColor::systemOrangeColor()),
            BadgeRank::Alarm => muted(&NSColor::systemRedColor()),
        }
    }

    fn fill(self) -> Retained<NSColor> {
        match self {
            BadgeRank::Quiet => NSColor::labelColor().colorWithAlphaComponent(0.06),
            BadgeRank::Notable => NSColor::labelColor().colorWithAlphaComponent(0.10),
            BadgeRank::Attention | BadgeRank::Alarm => self.ink().colorWithAlphaComponent(0.14),
        }
    }
}

/// Blend a system hue toward the foreground so it pastels on dark and deepens
/// on light. Resolves now; rebuild under the view's appearance if that changes.
fn muted(base: &NSColor) -> Retained<NSColor> {
    base.blendedColorWithFraction_ofColor(0.3, &NSColor::labelColor())
        .unwrap_or_else(|| base.retain())
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
            self.ivars().tone.borrow().rank().fill().setFill();
            path.fill();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn view_did_change_effective_appearance(&self) {
            self.paint_ink();
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
        this.paint_ink();
        this
    }

    /// Re-style in place (cheaper than replacing the view).
    pub fn set(&self, text: &str, tone: BadgeTone, tooltip: &str) {
        let iv = self.ivars();
        iv.label.setStringValue(&ns(text));
        *iv.tone.borrow_mut() = tone;
        self.setToolTip(Some(&ns(tooltip)));
        self.paint_ink();
    }

    /// Label ink is set here, not in `drawRect:`; blends snapshot at resolve.
    fn paint_ink(&self) {
        let rank = self.ivars().tone.borrow().rank();
        self.setNeedsDisplay(true);
        let label = self.ivars().label.clone();
        self.effectiveAppearance()
            .performAsCurrentDrawingAppearance(&RcBlock::new(move || {
                label.setTextColor(Some(&rank.ink()));
            }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use wtm_core::{WorktreeInfo, WorktreeStatus};

    fn clean_status() -> WorktreeStatus {
        WorktreeStatus {
            has_unstaged: false,
            has_staged: false,
            has_untracked: false,
            trunk_ref: "origin/main".into(),
            ahead_of_main: Some(0),
            behind_main: Some(0),
            unpushed: false,
            unpushed_count: 0,
            has_upstream: true,
        }
    }

    fn worktree(status: Option<WorktreeStatus>) -> WorktreeInfo {
        WorktreeInfo {
            path: "/w".into(),
            branch: Some("feature/thing".into()),
            head: "abc1234".into(),
            is_main: false,
            locked: false,
            prunable: false,
            status,
        }
    }

    fn ranks(w: &WorktreeInfo) -> Vec<BadgeRank> {
        badges_for(w, "main")
            .into_iter()
            .map(|(_, tone, _)| tone.rank())
            .collect()
    }

    fn coloured(w: &WorktreeInfo) -> usize {
        ranks(w)
            .into_iter()
            .filter(|r| matches!(r, BadgeRank::Attention | BadgeRank::Alarm))
            .count()
    }

    #[test]
    fn a_clean_in_sync_worktree_spends_no_colour() {
        assert_eq!(coloured(&worktree(Some(clean_status()))), 0);
    }

    #[test]
    fn distance_from_the_trunk_stays_quiet() {
        let mut s = clean_status();
        s.ahead_of_main = Some(4);
        s.behind_main = Some(264);
        assert!(ranks(&worktree(Some(s)))
            .iter()
            .all(|r| *r == BadgeRank::Quiet));
    }

    #[test]
    fn unpushed_commits_are_notable_rather_than_coloured() {
        let mut s = clean_status();
        s.unpushed = true;
        s.unpushed_count = 2;
        let w = worktree(Some(s));
        assert!(ranks(&w).contains(&BadgeRank::Notable));
        assert_eq!(coloured(&w), 0);
    }

    #[test]
    fn only_uncommitted_work_and_a_missing_folder_are_coloured() {
        let mut s = clean_status();
        s.has_staged = true;
        s.has_unstaged = true;
        s.has_untracked = true;
        assert_eq!(coloured(&worktree(Some(s))), 3);

        let mut gone = worktree(None);
        gone.prunable = true;
        assert_eq!(coloured(&gone), 1);

        // A row the app could not read is a gap in knowledge, not an alarm.
        assert_eq!(coloured(&worktree(None)), 0);
    }

    #[test]
    fn primary_and_locked_markers_are_labels_not_alerts() {
        let mut main = worktree(Some(clean_status()));
        main.is_main = true;
        main.locked = true;
        assert_eq!(coloured(&main), 0);
    }
}
