#!/bin/sh
# Regenerate the app icons from the Icon Composer source (build/AppIcon.icon).
#
# Requires Xcode 26+ (for actool's .icon support). Produces:
#   build/Assets.car  — full-resolution Liquid Glass icon used on macOS 26+
#                       (referenced via CFBundleIconName, shipped as an
#                       extraResource by electron-builder).
#   build/icon.icns   — legacy fallback for older macOS (actool caps this at
#                       256px; the crisp icon lives in Assets.car).
#   build/icon.png    — 256px render used for the dev-mode dock icon.
#
# Edit the icon in Icon Composer, save over build/AppIcon.icon, then run this.
set -eu

cd "$(dirname "$0")"
BUILD="$(pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

xcrun actool "$BUILD/AppIcon.icon" \
  --compile "$TMP" \
  --platform macosx \
  --minimum-deployment-target 12.0 \
  --app-icon AppIcon \
  --output-partial-info-plist "$TMP/partial.plist" \
  --output-format human-readable-text --errors --warnings

cp "$TMP/AppIcon.icns" "$BUILD/icon.icns"
cp "$TMP/Assets.car" "$BUILD/Assets.car"

# Extract the largest rendered size (256px) as the dev-mode dock PNG.
iconutil --convert iconset --output "$TMP/icon.iconset" "$BUILD/icon.icns"
cp "$TMP/icon.iconset/icon_128x128@2x.png" "$BUILD/icon.png"

echo "Regenerated build/{icon.icns,Assets.car,icon.png} from AppIcon.icon"
