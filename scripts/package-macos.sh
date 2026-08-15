#!/bin/bash
# 组装 dsh-launcher.app(macOS 打包;CI 与本地通用)
# 用法:scripts/package-macos.sh <version> <out-dir> <launcher-binary>
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
VERSION="${1:?version required}"
OUT="${2:?out dir required}"
BIN="${3:?launcher binary required}"
APP="$OUT/dsh-launcher.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/apps/$VERSION"
cp "$BIN" "$APP/Contents/MacOS/dsh-launcher"
cp "$ROOT/assets/icon.icns" "$APP/Contents/Resources/app.icns"
printf '{"current":"%s"}\n' "$VERSION" > "$APP/Contents/Resources/launcher.json"
cp -R "$ROOT/src" "$ROOT/public" "$ROOT/bin" "$ROOT/scripts" \
      "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/package.json" \
      "$APP/Contents/Resources/apps/$VERSION/"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>dsh-launcher</string>
  <key>CFBundleDisplayName</key><string>dsh-launcher</string>
  <key>CFBundleIdentifier</key><string>com.dshlauncher.app</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>dsh-launcher</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>app</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT License</string>
</dict>
</plist>
EOF

echo "已生成 $APP"
