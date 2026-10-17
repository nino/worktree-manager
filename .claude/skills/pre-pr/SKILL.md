---
name: pre-pr
description: Review your own change before publishing it. Use before opening a pull request, before marking a draft ready for review, and before pushing a large change to an open PR: run the checks, review the diff, fix what the review finds.
---

# Before a pull request

A reviewer's time costs more than yours, and a finding fixed before the PR
exists never shows up as a red check or a review thread. So every change gets
a code review from its own author before anyone else sees it.

Do this before `create_pull_request` (or `gh pr create`), before turning a
draft into a ready PR, and before pushing a change of more than a few lines to
a PR that is already open. A one-line fix to a review comment does not need
the whole pass; the checks in step 1 still apply.

## 1. Run the checks

From the repo root, on the committed state of the branch:

```sh
cargo fmt --check
xvfb-run -a cargo test            # Linux; plain `cargo test` on macOS
cargo check --target x86_64-pc-windows-gnu    # if a backend or wtm-app changed
cargo check --target aarch64-apple-darwin     # likewise
```

Fix anything that fails before reviewing: a review of code that does not build
wastes the review.

## 2. Review the diff

Review everything the PR will contain, not only the last commit:

```sh
git fetch origin main
git diff origin/main...HEAD
```

Use the `/code-review` skill on that diff where it is available (`/code-review
high` for anything beyond a small change). Where it is not, start a subagent
with no prior context, give it the diff and `CLAUDE.md`, and ask it for
correctness bugs only, each with the input or state that triggers it. A fresh
reader catches what the author's own reasoning skips over.

Whichever you use, check the change against this repo's rules in `CLAUDE.md`
as well, in particular:

- logic in the right crate (what the UI shows goes in `wtm-ui`, tested with
  `wtm_toolkit::headless`; pure helpers in `wtm-core`), and the dependency
  direction intact;
- no `cfg(target_os)` outside `wtm-app` and the backend crate gates;
- a new vocabulary element implemented in all three backends;
- nothing tested against the real config (`WTM_USER_DATA` and a throwaway
  `HOME`);
- comments that say why, not what.

## 3. Act on the findings

Each finding is a claim to check, not an order. For each one, trace a
realistic path from a caller or user action to the failure:

- **Real, and the fix is proportionate:** fix it, with a test where the code
  is testable. Re-run step 1.
- **Real but outside this change** (pre-existing, unrelated code): leave it,
  and mention it in the PR description so the reviewer can decide.
- **Not real, or the fix costs more than it prevents:** drop it, and say why
  in one line in the PR description if a reviewer would likely raise it too.

If the fixes were more than small, review the new diff again. Stop when a pass
finds nothing worth fixing.

## 4. Publish

Open the PR (or mark it ready). In the description, under a short "Review"
heading, say that the pre-PR review ran and list what it found that you left
unfixed, with the reason. If it found nothing, say so in one line.
