//! What the New Worktree sheet says about its fields, and what Create sends.
//! Both come from [`check`], so the note under the name and the button can
//! never disagree about where the branch will come from.

use crate::branch_name;
use crate::model::BranchLocation;
use crate::types::BranchSource;

/// The sheet's verdict on its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What Create sends, or why it is off. The reason is empty while no name
    /// has been typed: that is not wrong yet, only not a name.
    pub create: Result<BranchSource, String>,
    /// Said under the name when nothing is wrong.
    pub note: Option<String>,
    /// The branch will come from a remote, so no base ref applies to it.
    pub from_remote: bool,
}

/// Check the sheet's fields against where the last listing puts the branch.
/// `new_branch` is the mode; `base_ref` is only used in that mode, and an
/// empty one means the repo's trunk.
pub fn check(name: &str, new_branch: bool, base_ref: &str, place: &BranchLocation) -> Check {
    let (name, base_ref) = (name.trim(), base_ref.trim());
    let from_remote = matches!(
        place,
        BranchLocation::Remote(_) | BranchLocation::Remotes(_)
    );
    let refuse = |reason: String| Check {
        create: Err(reason),
        note: None,
        from_remote,
    };
    let accept = |source: BranchSource, note: Option<String>| Check {
        create: Ok(source),
        note,
        from_remote,
    };
    if name.is_empty() {
        return refuse(String::new());
    }
    if let Some(reason) = branch_name::problem(name) {
        return refuse(reason);
    }
    match place {
        // A branch that only a remote has is checked out from there in
        // either mode.
        BranchLocation::Remote(remote) => accept(
            BranchSource::Remote(remote.clone()),
            Some(format!("Branch will be pulled from {remote}.")),
        ),
        BranchLocation::Remotes(remotes) => refuse(format!(
            "Branch is on more than one remote: {}.",
            remotes.join(", ")
        )),
        _ if !new_branch => accept(BranchSource::Existing, None),
        _ if base_ref.is_empty() => accept(BranchSource::New { base_ref: None }, None),
        // git judges a base ref by the same rules as a branch name.
        _ => match branch_name::problem(base_ref) {
            Some(reason) => refuse(format!("Base ref: {reason}")),
            None => accept(
                BranchSource::New {
                    base_ref: Some(base_ref.to_string()),
                },
                None,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOWHERE: BranchLocation = BranchLocation::Nowhere;

    #[test]
    fn no_name_is_not_an_error_but_keeps_create_off() {
        let c = check("  ", true, "", &NOWHERE);
        assert_eq!(c.create, Err(String::new()));
        assert_eq!(c.note, None);
    }

    #[test]
    fn a_bad_name_says_why_before_anything_else() {
        let c = check("a b", true, "", &BranchLocation::Remote("origin".into()));
        assert_eq!(c.create, Err("A branch name cannot contain spaces.".into()));
    }

    #[test]
    fn a_branch_only_one_remote_has_is_pulled_in_either_mode() {
        let origin = BranchLocation::Remote("origin".into());
        for new_branch in [true, false] {
            // Even a base ref git would refuse: it does not apply.
            let c = check("review/x", new_branch, "a..b", &origin);
            assert_eq!(c.create, Ok(BranchSource::Remote("origin".into())));
            assert_eq!(
                c.note.as_deref(),
                Some("Branch will be pulled from origin.")
            );
            assert!(c.from_remote);
        }
    }

    #[test]
    fn a_branch_several_remotes_have_keeps_create_off() {
        let both = BranchLocation::Remotes(vec!["fork".into(), "origin".into()]);
        let c = check("x", true, "", &both);
        assert_eq!(
            c.create,
            Err("Branch is on more than one remote: fork, origin.".into())
        );
        assert!(c.from_remote);
    }

    #[test]
    fn a_new_branch_takes_the_base_ref_or_the_trunk() {
        assert_eq!(
            check("x", true, " origin/dev ", &NOWHERE).create,
            Ok(BranchSource::New {
                base_ref: Some("origin/dev".into())
            })
        );
        assert_eq!(
            check("x", true, "", &NOWHERE).create,
            Ok(BranchSource::New { base_ref: None })
        );
        assert_eq!(
            check("x", true, "a..b", &NOWHERE).create,
            Err("Base ref: A branch name cannot contain “..”.".into())
        );
    }

    #[test]
    fn existing_mode_ignores_the_base_ref() {
        let c = check("fix", false, "a..b", &BranchLocation::Local);
        assert_eq!(c.create, Ok(BranchSource::Existing));
        assert!(!c.from_remote);
    }
}
