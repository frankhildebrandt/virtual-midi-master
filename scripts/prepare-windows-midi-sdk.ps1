param(
    [string]$Root = (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
)

$ErrorActionPreference = "Stop"
$Packages = Join-Path $Root ".packages"
$MidiFeed = Join-Path $Packages "midi-feed"
$Projection = Join-Path $Packages "midi-projection"
$CppWinRtPackage = Join-Path $Packages "Microsoft.Windows.CppWinRT.3.0.260520.1"
$MidiPackage = Join-Path $Packages "Windows.Devices.Midi2.0.99.44-devpreview.4"
$MidiPackageVersion = "0.99.44-devpreview.4"
$MidiPackageUrl = "https://github.com/microsoft/MIDI/releases/download/inbox-dev-preview-4/Windows.Devices.Midi2.0.99.44-devpreview.4.nupkg"

nuget install Microsoft.Windows.CppWinRT -Version 3.0.260520.1 -OutputDirectory $Packages -NonInteractive | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "Installing Microsoft.Windows.CppWinRT 3.0.260520.1 failed with exit code $LASTEXITCODE"
}
New-Item -ItemType Directory -Force $MidiFeed | Out-Null
$MidiPackageArchive = Join-Path $MidiFeed "Windows.Devices.Midi2.$MidiPackageVersion.nupkg"
if (-not (Test-Path $MidiPackageArchive)) {
    Invoke-WebRequest -Uri $MidiPackageUrl -OutFile $MidiPackageArchive
}
nuget install Windows.Devices.Midi2 -Version $MidiPackageVersion -Source $MidiFeed -OutputDirectory $Packages -NonInteractive | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "Installing Windows.Devices.Midi2 $MidiPackageVersion failed with exit code $LASTEXITCODE"
}

$MidiMetadata = Get-ChildItem $MidiPackage -Recurse -Filter "Windows.Devices.Midi2.winmd" |
    Select-Object -First 1 -ExpandProperty FullName
$CppWinRtTool = Get-ChildItem $CppWinRtPackage -Recurse -Filter "cppwinrt.exe" |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $MidiMetadata -or -not $CppWinRtTool) {
    throw "Windows MIDI SDK metadata or C++/WinRT tooling was not found"
}

& $CppWinRtTool -input $MidiMetadata -output $Projection
if ($LASTEXITCODE -ne 0) {
    throw "Generating Windows MIDI SDK C++ projections failed with exit code $LASTEXITCODE"
}

$Headers = Join-Path $Projection "winrt"
if (-not (Test-Path (Join-Path $Headers "Windows.Devices.Midi2.h")) -or
    -not (Test-Path (Join-Path $Headers "base.h"))) {
    throw "Generated Windows MIDI SDK headers were not found"
}

$IncludePath = $Projection

Write-Output $IncludePath
