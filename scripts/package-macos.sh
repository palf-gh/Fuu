#!/bin/sh
# Build a release Fuu and pack it as dist/Fuu.app and dist/Fuu-<version>-macos-<arch>.zip.
# Unsigned and not notarised yet (see docs/plan.md, M5): on first launch,
# right-click the app and choose Open.
#
#   sh scripts/package-macos.sh
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
arch=$(uname -m)
dist="$root/dist"
app="$dist/Fuu.app"

cargo build -p fuu-app --release --locked

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/fuu "$app/Contents/MacOS/Fuu"
cp LICENSE README.md "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Fuu</string>
    <key>CFBundleDisplayName</key><string>Fuu</string>
    <key>CFBundleExecutable</key><string>Fuu</string>
    <key>CFBundleIdentifier</key><string>dev.fuu.Fuu</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleDevelopmentRegion</key><string>ja</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSHumanReadableCopyright</key><string>GPL-3.0-only</string>
</dict>
</plist>
PLIST

zip="$dist/Fuu-$version-macos-$arch.zip"
rm -f "$zip"
# ditto keeps the bundle structure and extended attributes intact.
(cd "$dist" && ditto -c -k --keepParent Fuu.app "$zip")
shasum -a 256 "$zip" | sed "s|$dist/||" > "$zip.sha256"
echo "$zip"
cat "$zip.sha256"
