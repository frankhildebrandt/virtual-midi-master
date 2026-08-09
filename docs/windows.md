# Windows 11 build and runtime

Windows builds require Windows 11 24H2/25H2 with Windows MIDI Services enabled and the matching Windows MIDI Services App SDK Runtime. The bridge checks `MidiApi::EnsureServiceAvailable()` and fails with an actionable message when activation is unavailable.

The C++ build needs generated headers for:

- `Microsoft.Windows.CppWinRT` (`3.0.260520.1` in the current official Microsoft sample)
- `Windows.Devices.Midi2` (`0.99.44-devpreview.4` in that sample)

The versions above are intentionally explicit. Update them only together with a Windows smoke-test pass because Microsoft MIDI SDK RC builds have introduced ABI changes.

Set `VMM_WINDOWS_MIDI_SDK_INCLUDE` to a semicolon-separated list of directories containing the generated `winrt/Windows.Devices.Midi2*.h` and standard C++/WinRT headers. `scripts/package-windows.ps1` downloads the pinned packages, discovers those directories and performs the x64 package build.

Required smoke tests on Windows:

1. Verify the SDK runtime and service status in the dashboard.
2. Create two virtual devices and find both directions in a Windows MIDI Services app, a 64-bit WinMM app and a 32-bit WinMM app.
3. Roundtrip Note, CC, Pitch Bend, Clock and SysEx messages in both directions.
4. Close the Tauri window and confirm ports remain available.
5. Sign out/in and confirm the engine returns through the HKCU Run entry.
