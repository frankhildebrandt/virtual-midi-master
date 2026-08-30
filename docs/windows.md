# Windows 11 build and runtime

Windows builds require Windows 11 24H2/25H2 with Windows MIDI Services enabled and the matching Windows MIDI Services App SDK Runtime. The bridge initializes the SDK through `MidiDesktopAppSdkInitializer`, checks `EnsureServiceAvailable()` and `MidiVirtualDeviceManager::IsTransportAvailable()`, and fails with an actionable message when activation is unavailable. Install the runtime from <https://aka.ms/midi>.

The C++ build needs generated headers for:

- `Microsoft.Windows.CppWinRT` (`3.0.260520.1`)
- `Microsoft.Windows.Devices.Midi2` (`1.0.17-rc.4.25`, downloaded from Microsoft's official MIDI GitHub `rc-4` release)

The versions above are intentionally explicit. Update them only together with a Windows smoke-test pass because Microsoft MIDI SDK RC builds have introduced ABI changes.

Set `VMM_WINDOWS_MIDI_SDK_INCLUDE` to a semicolon-separated list of directories containing the generated `winrt/Microsoft.Windows.Devices.Midi2*.h`, standard C++/WinRT headers, and `Microsoft.Windows.Devices.Midi2.Initialization.hpp`. `scripts/package-windows.ps1` downloads the pinned packages, discovers those directories and performs the x64 package build. Building with plain `cargo` on Windows without that environment variable is an error.

Required smoke tests on Windows:

1. Verify the SDK runtime and service status in the dashboard. If Runtime is missing, install the App SDK Runtime and confirm Windows 11 24H2/25H2.
2. Create two virtual devices and find both directions in a Windows MIDI Services app, a 64-bit WinMM app and a 32-bit WinMM app.
3. Roundtrip Note, CC, Pitch Bend, Clock and SysEx messages in both directions.
4. Close the Tauri window and confirm ports remain available.
5. Sign out/in and confirm the engine returns through the HKCU Run entry.
