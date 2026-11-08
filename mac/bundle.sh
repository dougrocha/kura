#!/usr/bin/env bash
# Build mac/build/Kura.app: the SwiftUI app with the kura server inside.
#   Kura.app/Contents/MacOS/Kura   the app
#   Kura.app/Contents/MacOS/kura-server   the server it starts (Bundle.main auxiliary executable)
# kura-server, not the kura CLI: it has no TUI, so it links only system libraries.
# (And `kura` would overwrite `Kura`: the filesystem ignores case.)
# Signed ad hoc, which is all macOS needs to run an app you built yourself.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release -p kura-server --bin kura-server
swift build -c release --package-path mac

app=mac/build/Kura.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "mac/.build/release/Kura" "$app/Contents/MacOS/Kura"
cp target/release/kura-server "$app/Contents/MacOS/kura-server"
sed "s/__VERSION__/$version/g" mac/Info.plist > "$app/Contents/Info.plist"

# Inside out: the nested binary first, then the app. Hardened runtime on both.
codesign --force --options runtime --timestamp=none --sign - "$app/Contents/MacOS/kura-server"
codesign --force --options runtime --timestamp=none --sign - "$app"
codesign --verify --strict --verbose=1 "$app"
echo "built $app ($version)"
