#!/bin/zsh
# Build a release binary and wrap it in a .app bundle at
# rust/target/bundle/Worktree Manager.app, reusing the icon assets the
# Electron build already generates (build/icon.icns, build/Assets.car).
#
# Usage: rust/scripts/bundle.sh [--install]   (--install copies to /Applications)
set -euo pipefail
here=${0:A:h}
root=$here/..
repo=$root/..
app="$root/target/bundle/Worktree Manager.app"

cargo build --release --manifest-path "$root/Cargo.toml"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/bundle/Info.plist" "$app/Contents/Info.plist"
cp "$root/target/release/worktree-manager" "$app/Contents/MacOS/worktree-manager"
[[ -f "$repo/build/icon.icns" ]] && cp "$repo/build/icon.icns" "$app/Contents/Resources/icon.icns"
[[ -f "$repo/build/Assets.car" ]] && cp "$repo/build/Assets.car" "$app/Contents/Resources/Assets.car"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Ad-hoc signature so macOS treats the bundle as a stable identity (needed for
# window-frame autosave and to avoid repeated "unidentified" prompts locally).
codesign --force --sign - "$app" >/dev/null 2>&1 || true

echo "built: $app"
if [[ "${1:-}" == "--install" ]]; then
  rm -rf "/Applications/Worktree Manager (Native).app"
  cp -R "$app" "/Applications/Worktree Manager (Native).app"
  echo "installed: /Applications/Worktree Manager (Native).app"
fi
