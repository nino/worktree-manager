//! The removals and insertions that turn one list into another, for updating
//! a list view in place instead of reloading it: rows present in both keep
//! their views.

use std::collections::HashSet;
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
/// elements: `(from, to)` pairs to apply one after another, each index into
/// the list as the moves before it left it, the way
/// `moveItemAtIndex:inParent:toIndex:inParent:` takes them. Every move is
/// upwards (`to < from`), so it reads the same whether `to` counts the moved
/// element or not. `None` when the lists do not hold the same elements.
/// Elements are expected to be unique within each list.
pub fn moves<T: Eq + Hash>(old: &[T], new: &[T]) -> Option<Vec<(usize, usize)>> {
    if old.len() != new.len() || old.iter().collect::<HashSet<_>>() != new.iter().collect() {
        return None;
    }
    let mut current: Vec<&T> = old.iter().collect();
    let mut out = Vec::new();
    for (to, x) in new.iter().enumerate() {
        let from = current.iter().position(|y| *y == x)?;
        if from != to {
            let moved = current.remove(from);
            current.insert(to, moved);
            out.push((from, to));
        }
    }
    Some(out)
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
        // Moving one down is every element after it moving up.
        assert_eq!(
            moves(&["a", "b", "c"], &["b", "c", "a"]),
            Some(vec![(1, 0), (2, 1)])
        );
    }

    #[test]
    fn different_elements_are_not_moves() {
        assert_eq!(moves(&["a", "b"], &["a", "c"]), None);
        assert_eq!(moves(&["a", "b"], &["a"]), None);
    }
}
