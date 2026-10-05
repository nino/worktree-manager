#!/bin/sh
# Claude Code on the web: a fresh container has Rust but not GTK's headers,
# which the Linux build needs, nor Xvfb, which running it there needs. Run by
# the SessionStart hook in .claude/settings.json; does nothing anywhere else.
[ "$CLAUDE_CODE_REMOTE" = "true" ] || exit 0
pkg-config --exists gtk4 && command -v xvfb-run >/dev/null && exit 0
export DEBIAN_FRONTEND=noninteractive
SUDO=; [ "$(id -u)" -eq 0 ] || SUDO=sudo
$SUDO apt-get update -qq >/dev/null 2>&1
$SUDO apt-get install -y -qq libgtk-4-dev xvfb >/dev/null 2>&1 ||
    echo "cloud-setup: could not install libgtk-4-dev and xvfb" >&2
exit 0
