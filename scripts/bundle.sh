#!/bin/zsh
# Build a release binary and wrap it in a .app bundle at
# target/bundle/Worktree Manager.app.
#
# Usage: scripts/bundle.sh [--install] [--universal] [--version X.Y.Z] [--build N]
#   --install     also copy the bundle to /Applications
#   --universal   build arm64 + x86_64 and lipo them together (what the
#                 release workflow ships; a local build only needs the host)
#   --version     stamp CFBundleShortVersionString — the version the updater
#                 compares, which for a beta reads e.g. 1.0.112-beta.91
#   --build       stamp CFBundleVersion, which Apple wants to be digits and
#                 dots only; defaults to --version
set -euo pipefail
here=${0:A:h}
root=$here/..
app="$root/target/bundle/Worktree Manager.app"

install=0
universal=0
version=""
build=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --install) install=1 ;;
    --universal) universal=1 ;;
    --version) version=${2:?--version needs a value}; shift ;;
    --build) build=${2:?--build needs a value}; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if (( universal )); then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --manifest-path "$root/Cargo.toml" --target "$target"
  done
  binary="$root/target/universal-worktree-manager"
  lipo -create -output "$binary" \
    "$root/target/aarch64-apple-darwin/release/worktree-manager" \
    "$root/target/x86_64-apple-darwin/release/worktree-manager"
else
  cargo build --release --manifest-path "$root/Cargo.toml"
  binary="$root/target/release/worktree-manager"
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/bundle/Info.plist" "$app/Contents/Info.plist"
cp "$binary" "$app/Contents/MacOS/worktree-manager"
[[ -f "$root/build/icon.icns" ]] && cp "$root/build/icon.icns" "$app/Contents/Resources/icon.icns"
[[ -f "$root/build/Assets.car" ]] && cp "$root/build/Assets.car" "$app/Contents/Resources/Assets.car"
printf 'APPL????' > "$app/Contents/PkgInfo"

if [[ -n "$version" ]]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$app/Contents/Info.plist"
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${build:-$version}" "$app/Contents/Info.plist"
fi

# Ad-hoc signature so macOS treats the bundle as a stable identity (needed for
# window-frame autosave and to avoid repeated "unidentified" prompts locally).
# The release workflow re-signs with a Developer ID afterwards.
codesign --force --sign - "$app" >/dev/null 2>&1 || true

echo "built: $app"
if (( install )); then
  rm -rf "/Applications/Worktree Manager.app"
  cp -R "$app" "/Applications/Worktree Manager.app"
  echo "installed: /Applications/Worktree Manager.app"
fi
