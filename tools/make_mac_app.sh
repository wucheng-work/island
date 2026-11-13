#!/bin/bash
# Build "Island For Mac.app" and its DMG from Poni's Mac binaries. Runs on a Mac (lipo, codesign, hdiutil).
#
#   tools/make_mac_app.sh <arm64 binary> <x86_64 binary | -> <IslandForMac.icns | -> <out dir>
#
# The app uses the configured local identity (ad-hoc if absent), so the DMG is a test build: macOS asks once, in
# System Settings > Privacy & Security > Open Anyway, before it first opens. The DMG's window says so
# (tools/make_mac_dmg.sh). tools/sign_mac_dmg.sh makes the signed, notarized DMG from this one.
set -euo pipefail

arm="$1"; intel="$2"; icns="$3"; out="$4"
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" \
  | python3 -c 'import sys, json; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "poni"))')"

mkdir -p "$out"
app="$out/Island For Mac.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# One binary for Apple Silicon and Intel Macs.
if [ "$intel" != "-" ]; then
  lipo -create -output "$app/Contents/MacOS/poni" "$arm" "$intel"
else
  cp "$arm" "$app/Contents/MacOS/poni"
fi
chmod +x "$app/Contents/MacOS/poni"
lipo -info "$app/Contents/MacOS/poni"

sed "s/@VERSION@/$version/g" "$root/installer/mac/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
if [ "$icns" != "-" ]; then
  cp "$icns" "$app/Contents/Resources/IslandForMac.icns"
fi

# 所选提示音随应用携带，避免依赖接收方系统的音效文件布局或版本。
cp -R "$root/assets/sounds" "$app/Contents/Resources/Sounds"

# 分享包同样保留源码与第三方组件的版权和许可说明。
cp "$root/LICENSE" "$root/THIRD-PARTY-NOTICES.txt" "$app/Contents/Resources/"

# What plays, from macOS's own now playing (browsers, Spotify, Music): the MediaRemote adapter's framework
# and script (third_party/mediaremote-adapter), which /usr/bin/perl runs for the app
# (crates/poni/src/mac/services/mediaremote.rs). MEDIAREMOTE_BUILD is its CMake build (mac.yml); without
# it the app reads Music and Spotify only.
if [ -n "${MEDIAREMOTE_BUILD:-}" ]; then
  mkdir -p "$app/Contents/Frameworks"
  cp -R "$MEDIAREMOTE_BUILD/MediaRemoteAdapter.framework" "$app/Contents/Frameworks/"
  cp "$root/third_party/mediaremote-adapter/bin/mediaremote-adapter.pl" "$app/Contents/Resources/"
  lipo -info "$app/Contents/Frameworks/MediaRemoteAdapter.framework/MediaRemoteAdapter"
fi

# Seal the bundle and adapter with the same identity on every local update.
bash "$root/tools/sign_mac_app.sh" "$app"

# The DMG: the installer window, with the test build's note on Open Anyway.
dmg="$out/Island-For-Mac-$version-unsigned.dmg"
bash "$root/tools/make_mac_dmg.sh" "$app" "$dmg" test
shasum -a 256 "$dmg"
echo "built $dmg"
