#!/bin/zsh
# Drive a debug build of the app with random key presses and clicks until it
# crashes. objc2 checks every message send's argument and return types only
# when debug assertions are on, so a wrong `msg_send!` signature panics in
# `cargo run` and passes silently in a release build. This is how to find
# those, and any other panic a person might reach by clicking.
#
# Usage: scripts/monkey.sh [--steps N] [--seed S] [--runs R | --minutes M]
#                          [--binary PATH] [--no-build] [--keep]
#   --steps   input events per run (default 600)
#   --seed    seed for the first run (default: random). The seed fixes the
#             sequence of choices, so it is the way to replay a crash; what
#             they land on depends on the app's state and timing, so a replay
#             can drift from the original
#   --runs    runs with consecutive seeds, stopping at the first crash
#             (default 1)
#   --minutes as many runs as fit in this many minutes, instead of --runs
#   --binary  drive this binary instead of building target/debug
#   --no-build  use target/debug/worktree-manager as it is
#   --keep    keep the sandbox of a run that did not crash (one that did is
#             always kept)
#   --sandbox   only make a sandbox and print how to run the app against it,
#               for trying something by hand on the same demo repos
#
# Each run gets a throwaway directory under $TMPDIR holding everything the app
# touches: its profile (WTM_USER_DATA), four demo repos with a local bare
# origin each, the worktrees root and a git config. The real profile under
# ~/Library/Application Support is never read. The app runs with
# WTM_NO_LAUNCH=1, so Open in Terminal, Reveal and Open in Editor only log.
#
# It needs Accessibility and Screen Recording permission for the terminal
# that runs it (System Settings → Privacy & Security). The mouse pointer is
# really moved, so leave the machine alone while it runs; a panel in the
# bottom-right corner shows how far it has got. The row buttons that copy a
# path or branch write to the clipboard; its text is put back when the run
# ends.
set -euo pipefail
here=${0:A:h}
root=${here:h}

steps=600
seed=""
runs=""
minutes=0
binary=""
build=1
keep=0
sandbox_only=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --steps) steps=${2:?--steps needs a value}; shift ;;
    --seed) seed=${2:?--seed needs a value}; shift ;;
    --runs) runs=${2:?--runs needs a value}; shift ;;
    --minutes) minutes=${2:?--minutes needs a value}; shift ;;
    --binary) binary=${2:?--binary needs a value}; shift ;;
    --no-build) build=0 ;;
    --keep) keep=1 ;;
    --sandbox) sandbox_only=1 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
[[ -n $seed ]] || seed=$(( RANDOM * 32768 + RANDOM ))
# 0 runs: as many as the minutes allow.
if [[ -z $runs ]]; then
  (( minutes )) && runs=0 || runs=1
fi

if (( sandbox_only )); then
  build=0
fi
if [[ -z $binary ]]; then
  binary=$root/target/debug/worktree-manager
  if (( build )); then
    # A debug build: the encoding checks this is here to trip are compiled
    # out of a release one.
    (cd "$root" && cargo build)
  fi
fi
# --sandbox makes demo repos and stops; it never runs the app.
(( sandbox_only )) || [[ -x $binary ]] || { echo "no binary at $binary" >&2; exit 2; }

# MARK: Sandbox

# Git for the setup and for the app: no user or system config, so no commit
# signing prompt, hook or credential helper comes from this machine's setup.
sandbox_git() {
  GIT_CONFIG_GLOBAL=$sandbox/gitconfig GIT_CONFIG_NOSYSTEM=1 git "$@" >/dev/null 2>&1
}

commit() {
  local repo=$1 file=$2 message=$3
  print -r -- "$message" >> "$repo/$file"
  sandbox_git -C "$repo" add "$file"
  sandbox_git -C "$repo" commit -q -m "$message"
}

# A repo with a bare origin, `main` pushed to it, and a few commits.
make_repo() {
  local name=$1 trunk=$2
  local repo=$sandbox/repos/$name origin=$sandbox/origins/$name.git
  sandbox_git init -q --bare -b "$trunk" "$origin"
  sandbox_git init -q -b "$trunk" "$repo"
  sandbox_git -C "$repo" remote add origin "$origin"
  commit "$repo" README.md "$name"
  commit "$repo" notes.txt "first note"
  sandbox_git -C "$repo" push -q -u origin "$trunk"
}

# The trees the app has to draw: every badge, both agent marks, names that
# are long or not ASCII, a detached HEAD, a locked and a missing worktree,
# and a conflict.
make_sandbox() {
  sandbox=$(mktemp -d "${TMPDIR:-/tmp}/wtm-monkey.XXXXXX")
  sandbox=${sandbox:A}
  mkdir -p "$sandbox/profile" "$sandbox/repos" "$sandbox/origins" "$sandbox/worktrees"
  cat > "$sandbox/gitconfig" <<EOF
[user]
	name = Monkey
	email = monkey@example.invalid
[init]
	defaultBranch = main
[pull]
	rebase = false
[commit]
	gpgsign = false
EOF

  local wt=$sandbox/worktrees
  make_repo alpha main
  local a=$sandbox/repos/alpha
  sandbox_git -C "$a" worktree add -q -b feature/login "$wt/alpha/feature-login"
  commit "$wt/alpha/feature-login" login.txt "log in"
  sandbox_git -C "$wt/alpha/feature-login" push -q -u origin feature/login
  commit "$wt/alpha/feature-login" login.txt "unpushed"
  sandbox_git -C "$a" worktree add -q -b claude/fix-the-crash-on-collapse "$wt/alpha/claude-fix"
  commit "$wt/alpha/claude-fix" fix.txt "fix"
  sandbox_git -C "$a" worktree add -q -b cursor/refactor "$wt/alpha/cursor-refactor"
  print staged > "$wt/alpha/cursor-refactor/staged.txt"
  sandbox_git -C "$wt/alpha/cursor-refactor" add staged.txt
  print more >> "$wt/alpha/cursor-refactor/notes.txt"
  print new > "$wt/alpha/cursor-refactor/untracked.txt"
  sandbox_git -C "$a" worktree add -q --detach "$wt/alpha/detached" HEAD~1
  sandbox_git -C "$a" worktree add -q -b fix/ünïcödé-İstanbul "$wt/alpha/unicode"
  sandbox_git -C "$a" worktree add -q -b a-branch-name-long-enough-to-run-off-the-end-of-a-narrow-window "$wt/alpha/long"
  sandbox_git -C "$a" worktree lock "$wt/alpha/long"
  sandbox_git -C "$a" worktree add -q -b gone "$wt/alpha/gone"
  rm -rf "$wt/alpha/gone"
  commit "$a" notes.txt "trunk moves on"

  # Many branches, for the picker's list and its filter.
  make_repo beta master
  local b=$sandbox/repos/beta
  local i
  for i in {1..40}; do
    sandbox_git -C "$b" branch "topic/$i-$(( i * 7919 % 1000 ))"
  done
  sandbox_git -C "$b" worktree add -q "$wt/beta/topic-1" "topic/1-919"
  sandbox_git -C "$b" worktree add -q -b claude/Straße "$wt/beta/strasse"

  # A worktree in the middle of a conflicted merge.
  make_repo "gamma repo" main
  local g="$sandbox/repos/gamma repo"
  sandbox_git -C "$g" worktree add -q -b clash "$wt/gamma repo/clash"
  commit "$wt/gamma repo/clash" notes.txt "theirs"
  commit "$g" notes.txt "ours"
  sandbox_git -C "$wt/gamma repo/clash" merge main || true

  # A configured repo whose folder is gone.
  cat > "$sandbox/profile/config.json" <<EOF
{
  "worktreesRoot": "$wt",
  "editorCommand": "true",
  "updateChannel": "stable",
  "repos": [
    { "id": "alpha", "name": "alpha", "path": "$sandbox/repos/alpha", "mainBranch": "main" },
    { "id": "beta", "name": "beta", "path": "$sandbox/repos/beta", "mainBranch": "master" },
    { "id": "gamma", "name": "gamma repo", "path": "$sandbox/repos/gamma repo", "mainBranch": "main" },
    { "id": "missing", "name": "missing", "path": "$sandbox/repos/missing", "mainBranch": "main" }
  ]
}
EOF
  # What the driver compares the config against after every step.
  cp "$sandbox/profile/config.json" "$sandbox/config.start.json"
}

if (( sandbox_only )); then
  make_sandbox
  print "Sandbox: $sandbox\n"
  print "WTM_USER_DATA='$sandbox/profile' WTM_NO_LAUNCH=1 \\"
  print "  GIT_CONFIG_GLOBAL='$sandbox/gitconfig' GIT_CONFIG_NOSYSTEM=1 cargo run"
  exit 0
fi

# MARK: One run

clipboard=$(pbpaste 2>/dev/null || true)
app_pid="" driver=""
# Ctrl-C included: leave neither the app nor the driver running.
clean_up() {
  [[ -n $driver ]] && kill $driver 2>/dev/null
  [[ -n $app_pid ]] && kill $app_pid 2>/dev/null
  print -rn -- "$clipboard" | pbcopy
}
trap clean_up EXIT
trap 'exit 130' INT TERM

# Runs the app in a fresh sandbox and drives it. Sets `outcome` to `clean`,
# `crash`, `quit`, `hang`, `aborted` or `driver-error`, and `report` to the
# sandbox, where everything from the run is.
run_once() {
  local run_seed=$1 run_number=$2
  make_sandbox
  local log=$sandbox/stderr.log actions=$sandbox/actions.log
  # Alternate appearances: the two draw with different colour blends.
  local appearance=light
  (( run_seed % 2 )) || appearance=dark
  print "seed $run_seed · $appearance · sandbox $sandbox"

  WTM_USER_DATA=$sandbox/profile WTM_NO_LAUNCH=1 WTM_APPEARANCE=$appearance \
    GIT_CONFIG_GLOBAL=$sandbox/gitconfig GIT_CONFIG_NOSYSTEM=1 \
    RUST_BACKTRACE=1 RUST_LOG=info \
    "$binary" > "$log" 2>&1 &
  local pid=$!
  app_pid=$pid

  osascript -l JavaScript "$here/monkey.js" "$pid" "$run_seed" "$steps" "$sandbox" "$actions" "$log" \
    "$run_number" "$runs" "$deadline" &
  driver=$!

  # Every step writes to the action log, and every step asks the app
  # something over Accessibility, which blocks while its main thread is stuck.
  # A log that stops moving is a hang. It starts once the window is up: a
  # new binary's first launch can take a while.
  local driver_status=0 last now
  while kill -0 $driver 2>/dev/null; do
    sleep 2
    [[ -s $actions ]] || continue
    last=$(stat -f %m "$actions")
    now=$(date +%s)
    if (( now - last > 30 )) && kill -0 $pid 2>/dev/null; then
      sample $pid 5 -file "$sandbox/sample.txt" >/dev/null 2>&1 || true
      kill $driver 2>/dev/null || true
      driver_status=3
      break
    fi
  done
  if (( driver_status != 3 )); then
    wait $driver || driver_status=$?
  fi

  local alive=1
  kill -0 $pid 2>/dev/null || alive=0
  if (( alive )); then
    kill $pid 2>/dev/null || true
  fi
  local exit_code=0
  wait $pid 2>/dev/null || exit_code=$?
  app_pid="" driver=""

  if grep -q "panicked at\|Terminating app due to uncaught exception" "$log"; then
    outcome=crash
  elif (( ! alive )); then
    # Gone without a panic. A signal (status 128 + its number) is a crash;
    # a clean exit is a quit, which nothing the driver sends can cause.
    (( exit_code == 0 )) && outcome=quit || outcome=crash
  elif (( driver_status == 3 )); then
    outcome=hang
  elif (( driver_status == 4 )); then
    outcome=aborted
  elif (( driver_status != 0 )); then
    outcome=driver-error
  else
    outcome=clean
  fi
  report=$sandbox
  if [[ $outcome == crash ]]; then
    print "\n\033[1mCRASH\033[0m (seed $run_seed, app exit status $exit_code)"
    grep -m1 -A30 "panicked at\|Terminating app due to uncaught exception" "$log" || tail -30 "$log"
    print "\nLast actions:"
    tail -15 "$actions"
  elif [[ $outcome == quit ]]; then
    print "\nThe app quit (exit status 0) without a panic. The driver never sends"
    print "⌘Q, so either someone quit it or the app quit by itself. Last actions:"
    tail -8 "$actions"
  elif [[ $outcome == hang ]]; then
    print "\n\033[1mHANG\033[0m (seed $run_seed): the app stopped answering; see $sandbox/sample.txt"
  elif [[ $outcome == aborted ]]; then
    print "\nStopped: $(tail -1 "$actions")"
  elif [[ $outcome == driver-error ]]; then
    print "\nThe driver failed (status $driver_status); see $actions"
  fi
  # Anything AppKit complained about is worth a look even without a crash.
  grep -i "exception\|inconsisten\|invalid\|assert" "$log" | grep -v "panicked at" | sort | uniq -c | head -10 || true
}

# MARK: Runs

outcome=clean
deadline=0
(( minutes )) && deadline=$(( $(date +%s) + minutes * 60 ))
n=0
while (( runs == 0 || n < runs )) && (( deadline == 0 || $(date +%s) < deadline )); do
  run_once $(( seed + n )) $(( n + 1 ))
  (( n += 1 ))
  if [[ $outcome != clean ]]; then
    print "\nEverything from this run is in $report"
    exit 1
  fi
  (( keep )) || rm -rf "$report"
done
if (( minutes )); then
  print "\n$n run(s) in $minutes min from seed $seed: no crash"
else
  print "\n$n run(s) of $steps steps from seed $seed: no crash"
fi
