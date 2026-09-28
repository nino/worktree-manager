//! The removals and insertions that turn one list into another, for updating
//! a list view in place instead of reloading it: rows present in both keep
//! their views.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Indices in the old list to remove, then indices in the new list to insert,
/// both ascending — the order `NSTableView` and `NSOutlineView` take them in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Splice {
    pub removed: Vec<usize>,
    pub inserted: Vec<usize>,
}

impl Splice {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.inserted.is_empty()
    }
}

/// How to turn `old` into `new` by removing and inserting elements alone, or
/// `None` when the elements they share are in a different order, which only
/// a move would fix. Elements are expected to be unique within each list.
pub fn splice<T: Eq + Hash>(old: &[T], new: &[T]) -> Option<Splice> {
    let in_old: HashSet<&T> = old.iter().collect();
    let in_new: HashSet<&T> = new.iter().collect();
    let kept_old = old.iter().filter(|x| in_new.contains(x));
    let kept_new = new.iter().filter(|x| in_old.contains(x));
    if !kept_old.eq(kept_new) {
        return None;
    }
    let absent = |list: &[T], other: &HashSet<&T>| -> Vec<usize> {
        list.iter()
            .enumerate()
            .filter(|(_, x)| !other.contains(x))
            .map(|(i, _)| i)
            .collect()
    };
    Some(Splice {
        removed: absent(old, &in_new),
        inserted: absent(new, &in_old),
    })
}

/// The moves that put `old` in `new`'s order, when the two hold the same
/// elements: `(from, to)` pairs to apply one after another, each taking the
/// element at `from` out and putting it back at `to`, both indices into the
/// list as the moves before it left it — the way
/// `moveItemAtIndex:inParent:toIndex:inParent:` takes them. The elements in
/// the longest run that is already in order stay put, so one element taken
/// somewhere else is one move. `None` when the lists do not hold the same
/// elements. Elements are expected to be unique within each list.
pub fn moves<T: Eq + Hash>(old: &[T], new: &[T]) -> Option<Vec<(usize, usize)>> {
    if old.len() != new.len() {
        return None;
    }
    let at: HashMap<&T, usize> = old.iter().enumerate().map(|(i, x)| (x, i)).collect();
    // Where each element of `new` was in `old`.
    let was: Vec<usize> = new
        .iter()
        .map(|x| at.get(x).copied())
        .collect::<Option<_>>()?;
    if was.iter().collect::<HashSet<_>>().len() != was.len() {
        return None;
    }
    let stays = longest_increasing(&was);
    let mut current: Vec<usize> = (0..old.len()).collect();
    let mut out = Vec::new();
    for (i, &x) in was.iter().enumerate() {
        if stays[i] {
            continue;
        }
        let from = current.iter().position(|&y| y == x)?;
        current.remove(from);
        // Right after the element before it in `new`, which is either one
        // that stays or one already put in place.
        let to = match i {
            0 => 0,
            _ => current.iter().position(|&y| y == was[i - 1])? + 1,
        };
        current.insert(to, x);
        out.push((from, to));
    }
    Some(out)
}

/// Which elements of `xs` make up a longest strictly increasing subsequence.
fn longest_increasing(xs: &[usize]) -> Vec<bool> {
    // `len[i]`: the longest run ending at `i`; `prev[i]`: the one before it.
    let mut len = vec![1usize; xs.len()];
    let mut prev = vec![None; xs.len()];
    for i in 0..xs.len() {
        for j in 0..i {
            if xs[j] < xs[i] && len[j] + 1 > len[i] {
                len[i] = len[j] + 1;
                prev[i] = Some(j);
            }
        }
    }
    let mut on = vec![false; xs.len()];
    let mut i = (0..xs.len()).max_by_key(|&i| len[i]);
    while let Some(k) = i {
        on[k] = true;
        i = prev[k];
    }
    on
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrower_filter_only_removes() {
        let s = splice(&["a", "b", "c", "d"], &["b", "d"]).unwrap();
        assert_eq!(s.removed, vec![0, 2]);
        assert!(s.inserted.is_empty());
    }

    #[test]
    fn a_wider_filter_only_inserts() {
        let s = splice(&["b", "d"], &["a", "b", "c", "d", "e"]).unwrap();
        assert!(s.removed.is_empty());
        assert_eq!(s.inserted, vec![0, 2, 4]);
    }

    #[test]
    fn removals_index_the_old_list_and_insertions_the_new() {
        let (old, new) = (["a", "b", "c"], ["b", "x", "y"]);
        let s = splice(&old, &new).unwrap();
        assert_eq!(s.removed, vec![0, 2]);
        assert_eq!(s.inserted, vec![1, 2]);
    }

    #[test]
    fn the_same_list_is_an_empty_splice() {
        assert!(splice(&[1, 2, 3], &[1, 2, 3]).unwrap().is_empty());
        assert!(splice::<u8>(&[], &[]).unwrap().is_empty());
        assert!(!splice(&[1], &[]).unwrap().is_empty());
    }

    #[test]
    fn a_reorder_has_no_splice() {
        assert_eq!(splice(&["a", "b", "c"], &["c", "b"]), None);
        assert_eq!(splice(&["a", "b"], &["b", "x", "a"]), None);
    }

    #[test]
    fn a_reorder_is_moves() {
        assert_eq!(moves(&["a", "b", "c"], &["a", "b", "c"]), Some(vec![]));
        assert_eq!(
            moves(&["a", "b", "c"], &["c", "a", "b"]),
            Some(vec![(2, 0)])
        );
        assert_eq!(
            moves(&["a", "b", "c"], &["b", "c", "a"]),
            Some(vec![(0, 2)])
        );
        let one_swap = moves(&["a", "b", "c", "d"], &["a", "c", "b", "d"]);
        assert_eq!(one_swap.unwrap().len(), 1);
        assert_eq!(moves(&["a", "b", "c"], &["c", "b", "a"]).unwrap().len(), 2);
    }

    #[test]
    fn different_elements_are_not_moves() {
        assert_eq!(moves(&["a", "b"], &["a", "c"]), None);
        assert_eq!(moves(&["a", "b"], &["a"]), None);
        assert_eq!(moves(&["a", "b"], &["a", "a"]), None);
    }
}
