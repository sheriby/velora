#!/bin/bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$REPO_ROOT/Cargo.toml")"
PACKAGE_WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/maksher-package.XXXXXX")"
OUTPUT_DIR="$REPO_ROOT/dist"
APP_BUNDLE="$PACKAGE_WORK_DIR/payload/maksher.app"
ICONSET_DIR="$PACKAGE_WORK_DIR/maksher.iconset"

cleanup() {
    rm -rf "$PACKAGE_WORK_DIR"
}
trap cleanup EXIT

mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources" \
    "$ICONSET_DIR" "$OUTPUT_DIR"

cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --profile fastdev
cp "$REPO_ROOT/target/fastdev/maksher" "$APP_BUNDLE/Contents/MacOS/maksher"
cp "$REPO_ROOT/resources/macos/Info.plist" "$APP_BUNDLE/Contents/Info.plist"

/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" \
    "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" \
    "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIconFile maksher" \
    "$APP_BUNDLE/Contents/Info.plist"

render_icon() {
    pixel_size="$1"
    output_name="$2"
    sips -s format png -z "$pixel_size" "$pixel_size" \
        "$REPO_ROOT/assets/icon/maksher-app.svg" \
        --out "$ICONSET_DIR/$output_name" >/dev/null
}

render_icon 16 icon_16x16.png
render_icon 32 icon_16x16@2x.png
render_icon 32 icon_32x32.png
render_icon 64 icon_32x32@2x.png
render_icon 128 icon_128x128.png
render_icon 256 icon_128x128@2x.png
render_icon 256 icon_256x256.png
render_icon 512 icon_256x256@2x.png
render_icon 512 icon_512x512.png
render_icon 1024 icon_512x512@2x.png
iconutil -c icns "$ICONSET_DIR" -o "$APP_BUNDLE/Contents/Resources/maksher.icns"

plutil -lint "$APP_BUNDLE/Contents/Info.plist"
test -x "$APP_BUNDLE/Contents/MacOS/maksher"

APP_OUTPUT="$OUTPUT_DIR/maksher.app"
PKG_OUTPUT="$OUTPUT_DIR/maksher-$VERSION.pkg"
rm -rf "$APP_OUTPUT"
rm -f "$PKG_OUTPUT"
ditto "$APP_BUNDLE" "$APP_OUTPUT"

pkgbuild \
    --root "$PACKAGE_WORK_DIR/payload" \
    --identifier app.maksher.editor \
    --version "$VERSION" \
    --install-location /Applications \
    "$PACKAGE_WORK_DIR/maksher-component.pkg"

sed "s/__MAKSHER_VERSION__/$VERSION/g" \
    "$REPO_ROOT/resources/macos/pkg/Distribution.xml" \
    > "$PACKAGE_WORK_DIR/Distribution.xml"
productbuild \
    --distribution "$PACKAGE_WORK_DIR/Distribution.xml" \
    --package-path "$PACKAGE_WORK_DIR" \
    "$PKG_OUTPUT"

pkgutil --expand-full "$PKG_OUTPUT" "$PACKAGE_WORK_DIR/expanded-package"
test -f "$PACKAGE_WORK_DIR/expanded-package/maksher-component.pkg/Payload/maksher.app/Contents/Info.plist"
echo "已生成：$APP_OUTPUT"
echo "已生成：$PKG_OUTPUT"
