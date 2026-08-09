$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:VMM_WINDOWS_MIDI_SDK_INCLUDE = & (Join-Path $Root "scripts/prepare-windows-midi-sdk.ps1") -Root $Root

cargo build --release -p virtual-midi-engine --bin virtual-midi-engine --target x86_64-pc-windows-msvc
New-Item -ItemType Directory -Force apps/desktop/src-tauri/bin | Out-Null
Copy-Item target/x86_64-pc-windows-msvc/release/virtual-midi-engine.exe apps/desktop/src-tauri/bin/virtual-midi-engine-x86_64-pc-windows-msvc.exe
Set-Location apps/desktop
npm install
npm run tauri build -- --target x86_64-pc-windows-msvc
