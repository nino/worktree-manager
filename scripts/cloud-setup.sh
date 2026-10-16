#!/bin/sh
# Claude Code on the web: a fresh container has Rust and Bun but not GTK's
# headers, which the Linux build needs, nor WebKitGTK's, which the Tauri app
# (crates/wtm-tauri) needs, nor Xvfb, which running either there needs. Run
# by the SessionStart hook in .claude/settings.json; does nothing anywhere
# else.
[ "$CLAUDE_CODE_REMOTE" = "true" ] || exit 0
ROOT=$(cd "$(dirname "$0")/.." && pwd)
if ! { pkg-config --exists gtk4 webkit2gtk-4.1 && command -v xvfb-run >/dev/null; }; then
    export DEBIAN_FRONTEND=noninteractive
    SUDO=; [ "$(id -u)" -eq 0 ] || SUDO=sudo
    $SUDO apt-get update -qq >/dev/null 2>&1
    $SUDO apt-get install -y -qq libgtk-4-dev libwebkit2gtk-4.1-dev xvfb >/dev/null 2>&1 ||
        echo "cloud-setup: could not install libgtk-4-dev, libwebkit2gtk-4.1-dev and xvfb" >&2
fi
# The Tauri app's frontend, which is built and tested with Bun.
command -v bun >/dev/null || export PATH="$HOME/.bun/bin:$PATH"
if ! command -v bun >/dev/null; then
    echo "cloud-setup: bun not found; the frontend in ui/ needs it (https://bun.sh)" >&2
elif [ ! -d "$ROOT/ui/node_modules" ]; then
    (cd "$ROOT/ui" && bun install --frozen-lockfile >/dev/null 2>&1) ||
        echo "cloud-setup: bun install in ui/ failed" >&2
fi
exit 0
