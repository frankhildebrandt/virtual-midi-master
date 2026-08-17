#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root_dir"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release -p virtual-midi-engine --bin virtual-midi-engine --target aarch64-apple-darwin
cargo build --release -p virtual-midi-engine --bin virtual-midi-engine --target x86_64-apple-darwin
mkdir -p apps/desktop/src-tauri/bin
cp target/aarch64-apple-darwin/release/virtual-midi-engine \
  apps/desktop/src-tauri/bin/virtual-midi-engine-aarch64-apple-darwin
cp target/x86_64-apple-darwin/release/virtual-midi-engine \
  apps/desktop/src-tauri/bin/virtual-midi-engine-x86_64-apple-darwin
lipo -create \
  target/aarch64-apple-darwin/release/virtual-midi-engine \
  target/x86_64-apple-darwin/release/virtual-midi-engine \
  -output apps/desktop/src-tauri/bin/virtual-midi-engine-universal-apple-darwin
cd apps/desktop
npm install
npm run tauri build -- --target universal-apple-darwin
codesign --force --deep --sign - "$root_dir/target/universal-apple-darwin/release/bundle/macos/Virtual MIDI Master.app"
