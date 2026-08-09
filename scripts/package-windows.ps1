$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Packages = Join-Path $Root ".packages"
nuget install Microsoft.Windows.CppWinRT -Version 3.0.260520.1 -OutputDirectory $Packages
nuget install Windows.Devices.Midi2 -Version 0.99.44-devpreview.4 -OutputDirectory $Packages

$Headers = Get-ChildItem $Packages -Recurse -Filter "Windows.Devices.Midi2.h" | ForEach-Object { Split-Path -Parent (Split-Path -Parent $_.FullName) }
$CppWinRt = Get-ChildItem $Packages -Recurse -Filter "base.h" | Where-Object { $_.FullName -match "CppWinRT" } | ForEach-Object { Split-Path -Parent (Split-Path -Parent $_.FullName) }
$env:VMM_WINDOWS_MIDI_SDK_INCLUDE = (@($Headers) + @($CppWinRt) | Select-Object -Unique) -join ";"
if (-not $env:VMM_WINDOWS_MIDI_SDK_INCLUDE) { throw "Windows MIDI SDK headers were not found" }

cargo build --release -p virtual-midi-engine --bin virtual-midi-engine --target x86_64-pc-windows-msvc
New-Item -ItemType Directory -Force apps/desktop/src-tauri/bin | Out-Null
Copy-Item target/x86_64-pc-windows-msvc/release/virtual-midi-engine.exe apps/desktop/src-tauri/bin/virtual-midi-engine-x86_64-pc-windows-msvc.exe
Set-Location apps/desktop
npm install
npm run tauri build -- --target x86_64-pc-windows-msvc
