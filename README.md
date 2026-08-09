# Virtual MIDI Master

Virtual MIDI Master is a Rust MIDI routing engine with a Tauri 2 configuration app. It exposes multiple virtual MIDI 1.0 input/output pairs, connects physical controllers and synths in a many-to-many patchbay, and applies ordered filters and mappings in real time.

## Current MVP

- macOS 12+: native CoreMIDI virtual sources/destinations, physical MIDI I/O, SysEx and hotplug recovery.
- Windows 11 24H2/25H2 x64: Windows MIDI Services virtual-device transport through a narrow C++/WinRT bridge, with WinMM/WinRT MIDI 1.0 compatibility ports. Physical ports use the Windows `midir` backend.
- Separate background engine, local user-only IPC, atomic versioned configuration, LaunchAgent/HKCU autostart, bounded live monitor and Tauri/React patchbay.
- Filters for Pitch Bend, poly/channel aftertouch, Clock, System Realtime and SysEx; channel and CC mapping.

MIDI 2.0 authoring, LFOs, sequence recording, a node editor, Windows 10 and public code-signing are intentionally outside this release.

## Develop

Requirements: Rust 1.85+, Node 20+, npm, and the platform build tools.

```sh
cargo test --workspace
# Optional acceptance soak (runs for ten minutes):
cargo test -p virtual-midi-core --test performance routes_ten_thousand_events_per_second_for_ten_minutes -- --ignored --exact
cd apps/desktop
npm install
npm run build
```

For Tauri development, prepare the sidecar first:

```sh
cargo build -p virtual-midi-engine --bin virtual-midi-engine
mkdir -p apps/desktop/src-tauri/bin
cp target/debug/virtual-midi-engine apps/desktop/src-tauri/bin/virtual-midi-engine-aarch64-apple-darwin
cd apps/desktop && npm run tauri dev
```

Use the actual Rust target triple in the sidecar filename. Packaging helpers are under `scripts/`.

## Runtime files

- Configuration: platform configuration directory under `virtual-midi-master/config.json`.
- Invalid configurations are preserved as `config.invalid-<timestamp>.json` and defaults are loaded.
- IPC: per-user Unix domain socket with mode `0600`, or a local-only Windows named pipe with an owner/system ACL.
- `VMM_CONFIG_DIR` selects an alternative configuration root for tests.
- `VMM_DISABLE_AUTOSTART=1` prevents LaunchAgent/Run-key registration.

See [architecture](docs/architecture.md) and [Windows build notes](docs/windows.md).
