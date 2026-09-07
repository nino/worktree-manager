//! Fuzzy (subsequence) matching for the branch pickers, with the same ranking
//! as the Electron app: matches that start earlier first, then tighter
//! matches, then shorter names, then the original order.

/// Where a query matched inside a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Character index of the first matched query character.
    pub start: usize,
    /// Distance between the first and last matched characters.
    pub span: usize,
    /// Character index of each matched query character, in order.
    pub positions: Vec<usize>,
}

/// Greedy, case-insensitive subsequence match: the leftmost position of each
/// query character in `candidate`, or `None` when the query is not a
/// subsequence. An empty query matches everything at position 0.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<Match> {
    let mut positions = Vec::with_capacity(query.chars().count());
    let mut chars = candidate.chars().flat_map(char::to_lowercase).enumerate();
    for qc in query.chars().flat_map(char::to_lowercase) {
        let (i, _) = chars.by_ref().find(|(_, c)| *c == qc)?;
        positions.push(i);
    }
    let start = positions.first().copied().unwrap_or(0);
    let span = positions.last().map(|l| l - start).unwrap_or(0);
    Some(Match {
        start,
        span,
        positions,
    })
}

/// Indices into `candidates` of those matching `query`, best first. A blank
/// query keeps every candidate in its original order.
pub fn fuzzy_filter(query: &str, candidates: &[String]) -> Vec<(usize, Match)> {
    let query = query.trim();
    let mut scored: Vec<(usize, Match)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| fuzzy_match(query, c).map(|m| (i, m)))
        .collect();
    if query.is_empty() {
        return scored;
    }
    scored.sort_by(|(ia, a), (ib, b)| {
        a.start
            .cmp(&b.start)
            .then(a.span.cmp(&b.span))
            .then(candidates[*ia].len().cmp(&candidates[*ib].len()))
            .then(ia.cmp(ib))
    });
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn matches_subsequences_case_insensitively() {
        let m = fuzzy_match("fl", "Feature/Login").unwrap();
        assert_eq!(m.positions, vec![0, 8]);
        assert_eq!((m.start, m.span), (0, 8));
        assert!(fuzzy_match("xyz", "main").is_none());
        assert_eq!(
            fuzzy_match("", "main").unwrap().positions,
            Vec::<usize>::new()
        );
    }

    #[test]
    fn ranks_earlier_tighter_shorter_then_original_order() {
        let branches = names(&[
            "main",
            "feature/login",
            "fix/login",
            "login",
            "claude/fix-login",
        ]);
        let ranked: Vec<&str> = fuzzy_filter("login", &branches)
            .into_iter()
            .map(|(i, _)| branches[i].as_str())
            .collect();
        // Greedy leftmost matching (as in the Electron app): the `l` in
        // `claude` starts that match early.
        assert_eq!(
            ranked,
            vec!["login", "claude/fix-login", "fix/login", "feature/login"]
        );
    }

    #[test]
    fn blank_query_keeps_order() {
        let branches = names(&["b", "a"]);
        let idx: Vec<usize> = fuzzy_filter("  ", &branches)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert_eq!(idx, vec![0, 1]);
    }
}
