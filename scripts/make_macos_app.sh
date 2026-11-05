#!/usr/bin/env bash
#
# Build a self contained macOS .app bundle from target/release/oculante.
#
# The release binary links nothing outside /usr/lib and /System (no Homebrew
# libraries, because this fork builds without the `heif`/`avif_native`
# features), so the bundle only needs the executable, an icon and a plist.
#
# Usage:  scripts/make_macos_app.sh
# Result: dist/Oculante.app  (+ dist/Oculante-<version>.zip)
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN="target/release/oculante"
APP="dist/Oculante.app"
ICON_SRC="res/icons/icon.png"

if [ ! -x "$BIN" ]; then
  echo "missing $BIN - run 'cargo build --release' first" >&2
  exit 1
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
BUILD="$(date +%Y%m%d.%H%M%S)"

# ---------------------------------------------------------------------------
# bundle skeleton
# ---------------------------------------------------------------------------
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/oculante"
chmod +x "$APP/Contents/MacOS/oculante"

# ---------------------------------------------------------------------------
# icon: png -> iconset -> icns (kept inside the project, removed afterwards)
# ---------------------------------------------------------------------------
ICONSET="dist/.Oculante.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z $double $double "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Oculante.icns"
rm -rf "$ICONSET"

# ---------------------------------------------------------------------------
# Info.plist
# ---------------------------------------------------------------------------
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" \
  packaging/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# Ad-hoc signature: enough for local use, no notarisation possible without a
# developer certificate.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || \
  echo "warning: ad-hoc codesign failed, the app still runs locally" >&2

# ---------------------------------------------------------------------------
# zip for sharing (keeps the bundle structure and the signature)
# ---------------------------------------------------------------------------
ZIP="dist/Oculante-${VERSION}.zip"
rm -f "$ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"

echo "app: $APP"
echo "zip: $ZIP"
echo
echo "linked libraries outside the system:"
if otool -L "$APP/Contents/MacOS/oculante" | grep -E '/opt/homebrew|/usr/local'; then
  echo "  ^^ WARNING: bundle is not self contained" >&2
else
  echo "  none"
fi
