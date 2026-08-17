# Virtual MIDI Master

Virtual MIDI Master is a Rust MIDI routing engine with a Tauri 2 configuration app. It exposes multiple virtual MIDI 1.0 input/output pairs, connects physical controllers and synths in a many-to-many patchbay, and applies ordered filters and mappings in real time.

## Current MVP

- macOS 12+: native CoreMIDI virtual sources/destinations, physical MIDI I/O, SysEx and hotplug recovery.
- Windows 11 24H2/25H2 x64: Windows MIDI Services virtual-device transport through a narrow C++/WinRT bridge, with WinMM/WinRT MIDI 1.0 compatibility ports. Physical ports use the Windows `midir` backend.
- Separate background engine, local user-only IPC, atomic versioned configuration, LaunchAgent/HKCU autostart, bounded live monitor and Tauri/React patchbay.
- Filters for Pitch Bend, poly/channel aftertouch, Clock, System Realtime and SysEx; channel and CC mapping.
- Ordered per-device MIDI effects, including Scale Helper and Sustain Helper with configurable harmony reset.

MIDI 2.0 authoring, LFOs, sequence recording, a node editor, Windows 10 and public code-signing are intentionally outside this release.

## Develop

Requirements: Rust 1.88+, Node 20+, npm, and the platform build tools.

```sh
make help
make setup
make dev
```

The Makefile is the central entry point for development, checks and packaging. Common commands are:

```sh
make dev-ui           # React/Vite only
make dev-engine       # Rust engine only
make preview-ui       # preview the built UI
make build            # engine and UI
make build-app        # native release bundle for this host
make check            # formatting, Clippy, tests and UI build
make test-soak        # optional ten-minute acceptance soak
make package          # universal macOS package or Windows x64 installer
```

`make prepare-sidecar` determines the Rust host triple automatically. The platform packaging helpers remain available under `scripts/` and are exposed as `make package-macos` and `make package-windows`.
On Windows, all Rust and Tauri development/build targets run through the pinned Windows MIDI Services SDK environment; `make setup-windows` can prepare it explicitly.

## Runtime files

- Configuration: platform configuration directory under `virtual-midi-master/config.json`.
- Invalid configurations are preserved as `config.invalid-<timestamp>.json` and defaults are loaded.
- IPC: per-user Unix domain socket with mode `0600`, or a local-only Windows named pipe with an owner/system ACL.
- `VMM_CONFIG_DIR` selects an alternative configuration root for tests.
- `VMM_DISABLE_AUTOSTART=1` prevents LaunchAgent/Run-key registration.

See [architecture](docs/architecture.md) and [Windows build notes](docs/windows.md).
