//! Property tests: random input for the pure parts of the core, looking for
//! panics and broken invariants that the example-based unit tests miss.
//! proptest shrinks a failure to a small input and prints it; that input then
//! belongs in a unit test next to the code it broke, as the ones these found
//! are.
//!
//! Each property runs 256 cases under a plain `cargo test`. For a longer hunt:
//!
//! ```sh
//! PROPTEST_CASES=100000 cargo test --release -p wtm-core --test properties
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use serde_json::json;
use wtm_core::branch_name;
use wtm_core::branch_tool::split_tool_prefix;
use wtm_core::command::build_command;
use wtm_core::config::ConfigStore;
use wtm_core::fuzzy::{fuzzy_filter, fuzzy_match};
use wtm_core::git::{
    nonempty_lines, parse_left_right_count, parse_refs, parse_status_porcelain_v2,
    parse_worktree_porcelain, GitError, ParsedWorktree,
};
use wtm_core::paths::{sanitize_repo_name, slugify_branch, tildify, worktree_path_for};
use wtm_core::repos::describe_add_failure;
use wtm_core::splice::{splice, Splice};
use wtm_core::ui_state::UiStateStore;
use wtm_core::update::{is_newer, Manifest};
use wtm_core::WindowFrame;
use wtm_platform::AppDirs;

// MARK: Inputs

/// Uniformly random Unicode, or text drawn mostly from the characters these
/// parsers split and trim on, with multi-byte ones mixed in. Uniform text
/// almost never puts a separator next to a multi-byte character, which is
/// where byte-index slicing goes wrong.
fn texty() -> impl Strategy<Value = String> {
    prop_oneof![any::<String>(), r"[ \t\r\n#?!.:u12+/\\\-a-zé€İ😀]{0,40}",]
}

/// One line of text: no line breaks, at least one character.
fn line() -> impl Strategy<Value = String> {
    "[^\n\r]{1,30}"
}

/// Candidate branch names: arbitrary, or built from the characters git allows
/// in a ref that `slugify_branch` does not keep.
fn branchy() -> impl Strategy<Value = String> {
    prop_oneof![any::<String>(), "[a-zA-Z0-9./_+#@é -]{0,12}"]
}

fn f64_edge() -> impl Strategy<Value = f64> {
    prop_oneof![
        any::<f64>(),
        -5000.0..5000.0f64,
        Just(f64::NAN),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(f64::MAX),
        Just(0.0),
    ]
}

fn any_frame() -> impl Strategy<Value = WindowFrame> {
    (f64_edge(), f64_edge(), f64_edge(), f64_edge()).prop_map(|(x, y, width, height)| WindowFrame {
        x,
        y,
        width,
        height,
    })
}

/// A display's visible frame, the size and position real ones have.
fn screen() -> impl Strategy<Value = WindowFrame> {
    (
        -6000.0..6000.0f64,
        -3000.0..3000.0f64,
        640.0..6000.0f64,
        480.0..3000.0f64,
    )
        .prop_map(|(x, y, width, height)| WindowFrame {
            x,
            y,
            width,
            height,
        })
}

/// A throwaway config directory, unique to this case.
fn scratch_dirs(what: &str) -> AppDirs {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("wtm-properties-{what}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    AppDirs {
        config_dir: root.join("config"),
        home: root.clone(),
        legacy_config_files: Vec::new(),
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn remove_scratch(dirs: &AppDirs) {
    let _ = std::fs::remove_dir_all(&dirs.home);
}

// MARK: Git parsers

/// One worktree as `git worktree list --porcelain` prints it.
fn render_worktree(w: &ParsedWorktree, locked_reason: &str, prunable_reason: &str) -> String {
    let mut out = format!("worktree {}\n", w.path);
    if w.bare {
        out.push_str("bare\n");
    } else {
        out.push_str(&format!("HEAD {}\n", w.head));
        match &w.branch {
            Some(b) => out.push_str(&format!("branch refs/heads/{b}\n")),
            None if w.detached => out.push_str("detached\n"),
            None => {}
        }
    }
    let with_reason = |key: &str, reason: &str| {
        if reason.is_empty() {
            format!("{key}\n")
        } else {
            format!("{key} {reason}\n")
        }
    };
    if w.locked {
        out.push_str(&with_reason("locked", locked_reason));
    }
    if w.prunable {
        out.push_str(&with_reason("prunable", prunable_reason));
    }
    out.push('\n');
    out
}

fn parsed_worktree() -> impl Strategy<Value = (ParsedWorktree, String, String)> {
    (
        line(),
        "[0-9a-f]{40}",
        prop::option::of(line()),
        any::<(bool, bool, bool, bool)>(),
        "[^\n\r]{0,20}",
        "[^\n\r]{0,20}",
    )
        .prop_map(
            |(path, head, branch, (detached, locked, bare, prunable), lr, pr)| {
                let w = if bare {
                    ParsedWorktree {
                        path,
                        bare,
                        locked,
                        prunable,
                        ..Default::default()
                    }
                } else {
                    ParsedWorktree {
                        path,
                        head,
                        detached: detached && branch.is_none(),
                        branch,
                        locked,
                        bare,
                        prunable,
                    }
                };
                (w, lr, pr)
            },
        )
}

/// Lines shaped like `git status --porcelain=v2 --branch`, with any value.
fn status_porcelain() -> impl Strategy<Value = String> {
    let key = prop::sample::select(vec![
        "# branch.oid ",
        "# branch.head ",
        "# branch.upstream ",
        "# branch.ab ",
        "# branch.ab +",
        "# ",
        "#",
        "1 ",
        "2 ",
        "u ",
        "? ",
        "! ",
        "",
    ]);
    prop::collection::vec((key, texty()), 0..12).prop_map(|lines| {
        lines
            .into_iter()
            .map(|(k, v)| format!("{k}{v}\n"))
            .collect()
    })
}

/// Lines shaped like `git worktree list --porcelain`, with any value.
fn worktree_porcelain() -> impl Strategy<Value = String> {
    let key = prop::sample::select(vec![
        "worktree ",
        "worktree",
        "HEAD ",
        "branch ",
        "branch refs/heads/",
        "detached",
        "locked ",
        "bare",
        "prunable",
        "",
    ]);
    prop::collection::vec((key, texty()), 0..16).prop_map(|lines| {
        lines
            .into_iter()
            .map(|(k, v)| format!("{k}{v}\n"))
            .collect()
    })
}

proptest! {
    #[test]
    fn the_git_parsers_take_any_text(text in texty()) {
        parse_worktree_porcelain(&text);
        parse_status_porcelain_v2(&text);
        parse_left_right_count(&text);
        parse_refs(&text);
        nonempty_lines(&text);
    }

    #[test]
    fn the_worktree_parser_takes_porcelain_shaped_nonsense(text in worktree_porcelain()) {
        for w in parse_worktree_porcelain(&text) {
            prop_assert!(!w.path.is_empty());
        }
    }

    #[test]
    fn a_worktree_listing_parses_back_to_what_was_listed(
        entries in prop::collection::vec(parsed_worktree(), 0..6),
    ) {
        let text: String = entries
            .iter()
            .map(|(w, locked, prunable)| render_worktree(w, locked, prunable))
            .collect();
        let expected: Vec<ParsedWorktree> = entries.into_iter().map(|(w, _, _)| w).collect();
        prop_assert_eq!(parse_worktree_porcelain(&text), expected);
    }

    #[test]
    fn the_status_parser_takes_porcelain_shaped_nonsense(text in status_porcelain()) {
        parse_status_porcelain_v2(&text);
    }

    #[test]
    fn status_reads_any_upstream_counts(ahead: u32, behind: u32) {
        let s = parse_status_porcelain_v2(&format!("# branch.ab +{ahead} -{behind}\n"));
        prop_assert_eq!((s.ahead_upstream, s.behind_upstream), (ahead, behind));
    }

    #[test]
    fn status_reads_the_index_and_worktree_columns(
        x in prop::sample::select(vec!['.', 'M', 'T', 'A', 'D', 'R', 'C']),
        y in prop::sample::select(vec!['.', 'M', 'T', 'A', 'D', 'R', 'C']),
        kind in prop::sample::select(vec!['1', '2']),
        path in line(),
    ) {
        let s = parse_status_porcelain_v2(&format!("{kind} {x}{y} N... 100644 100644 100644 a b {path}\n"));
        prop_assert_eq!(s.has_staged, x != '.');
        prop_assert_eq!(s.has_unstaged, y != '.');
    }

    #[test]
    fn left_right_counts_read_back(behind: u32, ahead: u32) {
        prop_assert_eq!(parse_left_right_count(&format!("{behind}\t{ahead}\n")), (behind, ahead));
    }

    #[test]
    fn refs_keep_every_real_branch_in_order(
        refs in prop::collection::vec(
            (
                prop::sample::select(vec!["heads", "remotes"]),
                "[A-Za-z0-9._/-]{1,20}",
                prop::option::of("refs/[a-z/]{1,20}"),
            ),
            0..10,
        ),
    ) {
        let text: String = refs
            .iter()
            .map(|(kind, name, symref)| {
                format!("refs/{kind}/{name}\t{name}\t{}\n", symref.as_deref().unwrap_or(""))
            })
            .collect();
        let real = || refs.iter().filter(|(_, _, symref)| symref.is_none());
        let candidates: Vec<String> = real().map(|(_, name, _)| name.clone()).collect();
        let remote: Vec<String> = real()
            .filter(|(kind, _, _)| *kind == "remotes")
            .map(|(_, name, _)| name.clone())
            .collect();
        prop_assert_eq!(parse_refs(&text), (candidates, remote));
    }
}

// MARK: Fuzzy matching

/// A candidate and a query made of some of its characters, in order, some of
/// them upper-cased: a query that has to match.
fn candidate_and_subsequence() -> impl Strategy<Value = (String, String)> {
    texty().prop_flat_map(|candidate| {
        let n = candidate.chars().count();
        (
            Just(candidate),
            prop::collection::vec(any::<(bool, bool)>(), n),
        )
            .prop_map(|(candidate, picks)| {
                let query = candidate
                    .chars()
                    .zip(picks)
                    .filter(|(_, (keep, _))| *keep)
                    .map(|(c, (_, upper))| if upper { c.to_ascii_uppercase() } else { c })
                    .collect();
                (candidate, query)
            })
    })
}

proptest! {
    #[test]
    fn match_positions_are_characters_of_the_candidate(query in texty(), candidate in texty()) {
        if let Some(m) = fuzzy_match(&query, &candidate) {
            let len = candidate.chars().count();
            prop_assert!(m.positions.windows(2).all(|w| w[0] < w[1]), "{:?}", m.positions);
            prop_assert!(m.positions.iter().all(|&p| p < len), "{:?} in {} characters", m.positions, len);
            prop_assert_eq!(m.start, m.positions.first().copied().unwrap_or(0));
            prop_assert_eq!(m.span, m.positions.last().map_or(0, |l| l - m.start));
        }
    }

    #[test]
    fn a_subsequence_always_matches((candidate, query) in candidate_and_subsequence()) {
        let m = fuzzy_match(&query, &candidate);
        prop_assert!(m.is_some());
        let m = m.unwrap();
        let len = candidate.chars().count();
        prop_assert!(m.positions.iter().all(|&p| p < len), "{:?} in {} characters", m.positions, len);
    }

    #[test]
    fn filtering_ranks_each_match_once(
        query in prop_oneof![texty(), "[a-zé ]{0,3}"],
        candidates in prop::collection::vec(texty(), 0..8),
    ) {
        let ranked = fuzzy_filter(&query, &candidates);
        let mut seen: Vec<usize> = ranked.iter().map(|(i, _)| *i).collect();
        let order = seen.clone();
        seen.sort_unstable();
        seen.dedup();
        prop_assert_eq!(seen.len(), ranked.len());
        for (i, m) in &ranked {
            prop_assert_eq!(Some(m.clone()), fuzzy_match(query.trim(), &candidates[*i]));
        }
        let matching = candidates.iter().filter(|c| fuzzy_match(query.trim(), c).is_some()).count();
        prop_assert_eq!(ranked.len(), matching);
        if query.trim().is_empty() {
            prop_assert_eq!(order, (0..candidates.len()).collect::<Vec<_>>());
        }
    }
}

// MARK: List splices

/// Two lists of distinct elements in any order.
fn distinct_lists() -> impl Strategy<Value = (Vec<u8>, Vec<u8>)> {
    let list = || {
        // A `BTreeSet`, not a `HashSet`: its order is the same in every run,
        // so a failing seed replays.
        prop::collection::btree_set(0u8..12, 0..10)
            .prop_map(|s| s.into_iter().collect::<Vec<_>>())
            .prop_shuffle()
    };
    (list(), list())
}

/// Two filters of one list, the way a search narrows and widens the rows.
fn two_filters() -> impl Strategy<Value = (Vec<u8>, Vec<u8>)> {
    prop::collection::vec(any::<(bool, bool)>(), 0..16).prop_map(|picks| {
        let pick = |which: fn(&(bool, bool)) -> bool| {
            (0u8..)
                .zip(&picks)
                .filter(|(_, p)| which(p))
                .map(|(i, _)| i)
                .collect()
        };
        (pick(|p| p.0), pick(|p| p.1))
    })
}

/// What a list view shows once `s` is applied to its `old` rows: the removals
/// by old index, then each insertion at its new index.
fn apply_splice(old: &[u8], new: &[u8], s: &Splice) -> Vec<u8> {
    let mut out: Vec<u8> = (0..)
        .zip(old)
        .filter(|(i, _)| !s.removed.contains(i))
        .map(|(_, x)| *x)
        .collect();
    for &i in &s.inserted {
        out.insert(i, new[i]);
    }
    out
}

proptest! {
    #[test]
    fn a_splice_turns_the_old_list_into_the_new(
        (old, new) in prop_oneof![distinct_lists(), two_filters()],
    ) {
        if let Some(s) = splice(&old, &new) {
            prop_assert!(s.removed.windows(2).all(|w| w[0] < w[1]), "{:?}", s.removed);
            prop_assert!(s.inserted.windows(2).all(|w| w[0] < w[1]), "{:?}", s.inserted);
            prop_assert_eq!(apply_splice(&old, &new, &s), new);
        }
    }

    #[test]
    fn two_filters_of_one_list_always_splice((old, new) in two_filters()) {
        prop_assert!(splice(&old, &new).is_some());
    }
}

// MARK: Paths and names

proptest! {
    #[test]
    fn a_branch_slug_is_one_plain_directory(branch in branchy()) {
        let slug = slugify_branch(&branch);
        prop_assert!(!slug.is_empty());
        prop_assert!(
            slug.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
            "{:?}", slug
        );
        prop_assert!(!slug.starts_with(['-', '.']) && !slug.ends_with('-'), "{:?}", slug);
        prop_assert!(!slug.contains("--"), "{:?}", slug);
        prop_assert_eq!(slugify_branch(&slug), slug.clone());
        let path = worktree_path_for("/root", "repo", &branch);
        prop_assert_eq!(path.parent(), Some(Path::new("/root/repo")), "{:?} for {:?}", path, branch);
        prop_assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(slug.as_str()));
    }

    #[test]
    fn a_repo_name_is_one_plain_directory(
        name in prop_oneof![any::<String>(), r"[a-z./\\ ~-]{0,10}"],
    ) {
        let clean = sanitize_repo_name(&name);
        let joined = Path::new("/root").join(&clean);
        prop_assert_eq!(joined.parent(), Some(Path::new("/root")), "{:?} from {:?}", clean, name);
        prop_assert_eq!(joined.file_name().and_then(|n| n.to_str()), Some(clean.as_str()));
        prop_assert!(!clean.starts_with('.'));
        // `update_repo` sanitises on every save, so a clean name must stay put.
        prop_assert_eq!(sanitize_repo_name(&clean), clean.clone());
    }

    #[test]
    fn tildify_only_abbreviates_the_home_directory(
        path in prop_oneof![texty(), "(/[a-zé~]{0,4}){0,4}"],
        home in prop_oneof![texty(), "(/[a-zé~]{0,4}){0,2}"],
    ) {
        let short = tildify(&path, &home);
        if short != path {
            prop_assert!(!home.is_empty());
            let rest = short.strip_prefix('~');
            prop_assert!(rest.is_some(), "{:?}", short);
            let rest = rest.unwrap();
            prop_assert!(rest.is_empty() || rest.starts_with('/'));
            prop_assert_eq!(format!("{home}{rest}"), path);
        }
    }

    #[test]
    fn everything_under_home_is_abbreviated(
        home in "(/[a-zé ]{1,8}){1,3}",
        rest in "(/[^/]{1,8}){0,3}",
    ) {
        prop_assert_eq!(tildify(&format!("{home}{rest}"), &home), format!("~{rest}"));
    }

    #[test]
    fn an_agent_prefix_splits_off_exactly(
        branch in prop_oneof![any::<String>(), "(claude|cursor|Claude)?/?[a-zé/]{0,6}"],
    ) {
        if let Some((tool, rest)) = split_tool_prefix(&branch) {
            prop_assert!(!rest.is_empty());
            prop_assert_eq!(format!("{}{rest}", tool.prefix()), branch);
        }
    }
}

proptest! {
    // Each case starts a shell, so fewer of them by default. PROPTEST_CASES
    // wins over this, so the deep run in the header above starts one shell
    // per case — 100,000 of them, a few minutes.
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Paths are user-controlled (a folder name, a branch slug), and the
    /// editor command runs through a shell.
    #[test]
    fn a_path_reaches_the_command_verbatim(
        path in prop_oneof!["[^\\x00]{0,16}", r#"[ '"\\$`;{}()a-z]{0,16}"#],
        placeholder: bool,
    ) {
        let command = if placeholder { "printf %s {path}" } else { "printf %s" };
        let line = build_command(command, &path);
        let out = Command::new("/bin/sh").args(["-c", &line]).output().unwrap();
        prop_assert!(out.status.success(), "{}", line);
        prop_assert_eq!(String::from_utf8_lossy(&out.stdout), path.as_str());
    }
}

// MARK: Branch names

/// Names made of the pieces git's rules are about (dots, slashes, `.lock`,
/// `@{`, a leading dash, HEAD, the forbidden characters), so most land on a
/// boundary, or of arbitrary characters.
fn branch_name_candidates() -> impl Strategy<Value = String> {
    let piece = prop::sample::select(vec![
        "a", "é", "😀", "x-y", ".", "..", "/", "//", ".lock", "lock", "@", "{", "@{", "-", " ",
        "~", "^", ":", "?", "*", "[", "]", "\\", "\t", "\u{7f}", "HEAD",
    ]);
    prop_oneof![
        prop::collection::vec(piece, 1..6).prop_map(|pieces| pieces.concat()),
        "[^\\x00]{1,12}",
    ]
}

proptest! {
    /// Each case runs git. Outside any repository, so git does not read
    /// `@{-1}` as the branch checked out before.
    #[test]
    fn branch_names_are_judged_as_git_judges_them(name in branch_name_candidates()) {
        let git = Command::new("git")
            .args(["check-ref-format", "--branch", &name])
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        let problem = branch_name::problem(&name);
        prop_assert_eq!(problem.is_none(), git.status.success(), "{:?}: {:?}", name, problem);
    }
}

// MARK: Updates and messages

fn versiony() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        r"v?[0-9]{1,3}(\.[0-9]{1,25}){0,3}(-beta\.[0-9]{1,4})?",
    ]
}

fn manifest_json() -> impl Strategy<Value = String> {
    (
        prop_oneof![texty(), "[0-9.]{0,8}"],
        "(https?://)?[a-z./]{0,20}",
        "[0-9a-fA-Fx]{60,66}",
    )
        .prop_map(|(version, url, sha256)| {
            json!({ "version": version, "url": url, "sha256": sha256 }).to_string()
        })
}

proptest! {
    #[test]
    fn version_order_is_strict(a in versiony(), b in versiony()) {
        prop_assert!(!is_newer(&a, &a));
        prop_assert!(!(is_newer(&a, &b) && is_newer(&b, &a)));
    }

    #[test]
    fn a_beta_sits_between_its_stable_build_and_the_next(n in 0..u64::MAX, run in 1..u32::MAX) {
        let (stable, next, beta) = (
            format!("1.0.{n}"),
            format!("1.0.{}", n + 1),
            format!("1.0.{n}-beta.{run}"),
        );
        prop_assert!(is_newer(&next, &stable));
        prop_assert!(is_newer(&beta, &stable));
        prop_assert!(is_newer(&next, &beta));
    }

    #[test]
    fn a_manifest_is_only_accepted_when_it_can_be_acted_on(
        json in prop_oneof![texty(), manifest_json()],
    ) {
        if let Ok(m) = Manifest::parse(&json) {
            prop_assert!(m.url.starts_with("https://"));
            prop_assert!(m.sha256.len() == 64 && m.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            prop_assert!(!m.version.trim().is_empty());
        }
    }

    #[test]
    fn an_add_failure_is_one_line_of_text(
        message in prop_oneof![texty(), texty().prop_map(|s| format!("git rev-parse failed: {s}"))],
        stderr in texty(),
    ) {
        let err = GitError { message, cwd: String::new(), args: Vec::new(), stderr };
        let text = describe_add_failure(&err);
        prop_assert!(!text.trim().is_empty() && !text.contains('\n'), "{:?}", text);
    }
}

// MARK: Files on disk

fn config_json() -> impl Strategy<Value = String> {
    let repo = (
        "[a-z0-9]{0,3}",
        prop_oneof![texty(), r"[a-z./\\ ]{0,10}"],
        texty(),
        texty(),
    );
    (
        texty(),
        texty(),
        prop::option::of(texty()),
        prop::collection::vec(repo, 0..4),
    )
        .prop_map(|(root, editor, channel, repos)| {
            let repos: Vec<_> = repos
                .into_iter()
                .map(|(id, name, path, main)| {
                    json!({ "id": id, "name": name, "path": path, "mainBranch": main })
                })
                .collect();
            let mut config =
                json!({ "worktreesRoot": root, "editorCommand": editor, "repos": repos });
            if let Some(channel) = channel {
                config["updateChannel"] = json!(channel);
            }
            config.to_string()
        })
}

fn ui_state_json() -> impl Strategy<Value = String> {
    (
        prop::option::of((f64_edge(), f64_edge(), f64_edge(), f64_edge())),
        f64_edge(),
        prop::option::of((texty(), prop::option::of(texty()))),
        prop::collection::vec(texty(), 0..3),
    )
        .prop_map(|(window, scroll, focus, collapsed)| {
            // serde_json writes a non-finite number as null, which is one
            // more shape of broken file worth reading.
            json!({
                "window": window.map(|(x, y, width, height)| json!({ "x": x, "y": y, "width": width, "height": height })),
                "scroll": scroll,
                "focus": focus.map(|(repo_id, path)| json!({ "repoId": repo_id, "worktreePath": path })),
                "collapsedRepos": collapsed,
            })
            .to_string()
        })
}

fn config_file(dirs: &AppDirs, name: &str) -> PathBuf {
    dirs.config_dir.join(name)
}

proptest! {
    #[test]
    fn any_config_file_loads_and_saves_back_the_same(
        bytes in prop_oneof![
            any::<Vec<u8>>(),
            texty().prop_map(String::into_bytes),
            config_json().prop_map(String::into_bytes),
        ],
    ) {
        let dirs = scratch_dirs("config");
        write_file(&config_file(&dirs, "config.json"), &bytes);
        let loaded = ConfigStore::load(&dirs).config().clone();
        let again = ConfigStore::load(&dirs).config().clone();
        remove_scratch(&dirs);
        prop_assert_eq!(&loaded, &again);
        // A display name doubles as a directory under the worktrees root, and
        // a hand-edited or imported file has not been through `add_repo`.
        for repo in &loaded.repos {
            let joined = Path::new("/root").join(&repo.name);
            prop_assert_eq!(joined.parent(), Some(Path::new("/root")), "{:?}", repo.name);
        }
    }

    #[test]
    fn any_ui_state_file_loads(
        bytes in prop_oneof![
            any::<Vec<u8>>(),
            ui_state_json().prop_map(String::into_bytes),
        ],
    ) {
        let dirs = scratch_dirs("ui-state");
        write_file(&config_file(&dirs, "ui-state.json"), &bytes);
        let state = UiStateStore::load(&dirs).state().clone();
        remove_scratch(&dirs);
        prop_assert!(state.scroll.is_finite());
    }
}

// MARK: Window frames

proptest! {
    #[test]
    fn only_a_sane_frame_is_ever_usable(
        frame in any_frame(),
        screens in prop::collection::vec(any_frame(), 0..3),
    ) {
        if frame.is_usable_on(&screens) {
            prop_assert!([frame.x, frame.y, frame.width, frame.height].iter().all(|v| v.is_finite()));
            prop_assert!(frame.width >= 1.0 && frame.height >= 1.0);
        }
    }

    #[test]
    fn a_frame_inside_a_screen_is_usable(
        screen in screen(),
        (fx, fy, fw, fh) in (0.0..1.0f64, 0.0..1.0f64, 0.0..1.0f64, 0.0..1.0f64),
    ) {
        let width = (screen.width * fw).max(1.0);
        let height = (screen.height * fh).max(1.0);
        let frame = WindowFrame {
            x: screen.x + (screen.width - width) * fx,
            y: screen.y + (screen.height - height) * fy,
            width,
            height,
        };
        prop_assert!(frame.is_usable_on(&[screen]), "{:?} on {:?}", frame, screen);
    }

    #[test]
    fn a_frame_past_every_screen_is_not(
        screens in prop::collection::vec(screen(), 1..3),
        gap in 0.0..10_000.0f64,
        (width, height) in (1.0..5000.0f64, 1.0..5000.0f64),
        y in -5000.0..5000.0f64,
    ) {
        let right = screens.iter().map(|s| s.x + s.width).fold(f64::MIN, f64::max);
        let frame = WindowFrame { x: right + gap, y, width, height };
        prop_assert!(!frame.is_usable_on(&screens));
    }
}
