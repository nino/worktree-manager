#!/bin/sh
# Photograph the Tauri app (crates/wtm-tauri) on Linux, under Xvfb, against a
# throwaway sandbox of demo repos: never the real config or the real HOME.
#
#   scripts/tauri-screenshot.sh out.png            # light
#   WTM_APPEARANCE=dark scripts/tauri-screenshot.sh out.png
#   WTM_SANDBOX=dir scripts/tauri-screenshot.sh …  # reuse your own sandbox
#
# Builds the frontend and the debug binary first. Xvfb's screen is the
# window's own size and there is no window manager, so the PNG is the
# window's content, without a title bar.
set -eu
OUT=$(realpath -m "${1:?usage: $0 out.png}")
ROOT=$(cd "$(dirname "$0")/.." && pwd)

(cd "$ROOT/ui" && { [ -d node_modules ] || bun install --frozen-lockfile --silent; } && bun run --silent build >/dev/null)
cargo build -q -p wtm-tauri --manifest-path "$ROOT/Cargo.toml"

if [ -n "${WTM_SANDBOX:-}" ]; then
  S=$WTM_SANDBOX
else
  S=$(mktemp -d)
  trap 'rm -rf "$S"' EXIT
  H=$S/home
  mkdir -p "$H/src" "$H/worktrees/app" "$S/data"
  g() { d=$1; shift; git -C "$d" -c user.email=demo@example.com -c user.name=Demo "$@"; }
  git init -q -b main "$H/src/app"
  echo "# app" > "$H/src/app/README.md"
  g "$H/src/app" add README.md && g "$H/src/app" commit -q -m init
  g "$H/src/app" worktree add -q -b claude/tauri-rewrite "$H/worktrees/app/claude-tauri-rewrite"
  g "$H/src/app" worktree add -q -b fix-login "$H/worktrees/app/fix-login"
  echo change >> "$H/worktrees/app/claude-tauri-rewrite/README.md"
  echo new > "$H/worktrees/app/fix-login/notes.txt"
  git init -q -b main "$H/src/site"
  g "$H/src/site" commit -q --allow-empty -m init
  cat > "$S/data/config.json" <<EOF
{"repos":[
  {"id":"r1","name":"app","path":"$H/src/app","mainBranch":"main","initCommand":""},
  {"id":"r2","name":"site","path":"$H/src/site","mainBranch":"main","initCommand":""}],
 "worktreesRoot":"$H/worktrees","editorCommand":"true"}
EOF
fi

# WebKitGTK follows GTK's theme for prefers-color-scheme.
case "${WTM_APPEARANCE:-light}" in
  dark) export GTK_THEME=Adwaita:dark ;;
esac

LOG=$(mktemp)
HOME="$S/home" WTM_USER_DATA="$S/data" WTM_NO_LAUNCH=1 RUST_LOG=info \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  xvfb-run -a -s "-screen 0 880x640x24" sh -c '
    "$1" >"$2" 2>&1 &
    app=$!
    # Shown once the page has painted; then give git a moment to list.
    for _ in $(seq 100); do grep -q "window shown" "$2" && break; sleep 0.1; done
    sleep 2
    import -window root "$3"
    kill $app
  ' sh "$ROOT/target/debug/worktree-manager-tauri" "$LOG" "$OUT"
grep "window shown" "$LOG" || { cat "$LOG"; exit 1; }
rm -f "$LOG"
echo "$OUT"
