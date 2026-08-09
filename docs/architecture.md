# Architecture

The workspace has four deliberately narrow layers:

1. `virtual-midi-core` owns the stable JSON model, validation, cycle detection and allocation-light compiled routing rules.
2. `virtual-midi-platform` owns CoreMIDI, Windows MIDI Services and physical endpoint lifetimes. Callbacks only copy into a bounded channel.
3. `virtual-midi-engine` owns the routing worker, transactional configuration, statistics, autostart and local IPC.
4. `apps/desktop` is a replaceable Tauri/React client. It never owns a MIDI endpoint.

Endpoint IDs use `physical:input|output:<platform-port-id>` for physical ports and `virtual:<uuid>:in|out` for virtual devices. The platform port ID is CoreMIDI's unique endpoint identifier on macOS and the WinRT device ID on Windows. `in` is data sent by another application into Virtual MIDI Master; `out` is data emitted to another application.

An apply operation compiles and validates the new graph before it is accepted. Route, rule, autostart, and other non-port changes atomically switch the compiled router without interrupting existing endpoint objects. A virtual-device topology change rebuilds the affected platform objects and rolls back to the previous topology if preparation or persistence fails. The JSON is written through a synced temporary file and atomically replaced. The routing worker never performs persistence from a MIDI callback.

The Windows C++/WinRT bridge is a platform binding, not a second routing implementation. It creates bidirectional UMP virtual devices that declare MIDI 1.0 capability, requests classic compatibility ports, and converts complete MIDI 1.0 byte messages at the ABI boundary.
