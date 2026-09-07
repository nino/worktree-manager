#!/bin/zsh
# Build a release binary and wrap it in a .app bundle at
# target/bundle/Worktree Manager.app.
#
# Usage: scripts/bundle.sh [--install]   (--install copies to /Applications)
set -euo pipefail
here=${0:A:h}
root=$here/..
app="$root/target/bundle/Worktree Manager.app"

cargo build --release --manifest-path "$root/Cargo.toml"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/bundle/Info.plist" "$app/Contents/Info.plist"
cp "$root/target/release/worktree-manager" "$app/Contents/MacOS/worktree-manager"
[[ -f "$root/build/icon.icns" ]] && cp "$root/build/icon.icns" "$app/Contents/Resources/icon.icns"
[[ -f "$root/build/Assets.car" ]] && cp "$root/build/Assets.car" "$app/Contents/Resources/Assets.car"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Ad-hoc signature so macOS treats the bundle as a stable identity (needed for
# window-frame autosave and to avoid repeated "unidentified" prompts locally).
# Release builds are signed properly by the workflow (.github/workflows/release.yml).
codesign --force --sign - "$app" >/dev/null 2>&1 || true

echo "built: $app"
if [[ "${1:-}" == "--install" ]]; then
  rm -rf "/Applications/Worktree Manager.app"
  cp -R "$app" "/Applications/Worktree Manager.app"
  echo "installed: /Applications/Worktree Manager.app"
fi
