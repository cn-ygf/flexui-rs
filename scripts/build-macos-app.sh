#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
app_name="FlexUI Examples"
bundle_dir="$repo_dir/target/release/bundle/macos/$app_name.app"
contents_dir="$bundle_dir/Contents"

cd "$repo_dir"
cargo build --release -p flexui-examples

mkdir -p "$contents_dir/MacOS" "$contents_dir/Resources"
cp -f "target/release/flexui-examples" "$contents_dir/MacOS/flexui-examples"
cp -f "crates/flexui-examples/assets/app.icns" "$contents_dir/Resources/AppIcon.icns"

cat > "$contents_dir/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>FlexUI Examples</string>
  <key>CFBundleExecutable</key>
  <string>flexui-examples</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleIdentifier</key>
  <string>io.flexui.examples</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>FlexUI Examples</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

chmod +x "$contents_dir/MacOS/flexui-examples"
codesign --force --deep --sign - "$bundle_dir"
echo "$bundle_dir"
