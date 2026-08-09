# Virtual MIDI Master MVP specification

The MVP provides a separately running Rust engine and Tauri 2 configuration client for macOS 12+ and Windows 11 24H2/25H2 x64. It exposes multiple system-visible MIDI 1.0 input/output pairs, routes physical and virtual endpoints many-to-many, supports complete MIDI 1.0 including SysEx, and applies ordered message filters, channel mapping and CC mapping.

Configuration is versioned and transactional. Enabled route cycles are rejected, offline devices remain identifiable, callbacks are bounded and non-blocking, and the engine remains alive when the UI exits. It starts after user login without administrator rights. Local IPC must not expose a TCP listener and must be restricted to the current user.

The UI provides runtime status, endpoint state, a MIDI monitor, virtual-device CRUD, patchbay route CRUD, ordered rule editing, autostart and diagnostics. LFOs, recording, MIDI 2.0 authoring, a node editor, Windows 10, driver-manager hardware nodes and public signing/update infrastructure are excluded.
