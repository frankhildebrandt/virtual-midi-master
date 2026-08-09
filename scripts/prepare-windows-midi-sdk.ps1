param(
    [string]$Root = (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
)

$ErrorActionPreference = "Stop"
$Packages = Join-Path $Root ".packages"
$CppWinRtPackage = Join-Path $Packages "Microsoft.Windows.CppWinRT.3.0.260520.1"
$MidiPackage = Join-Path $Packages "Windows.Devices.Midi2.0.99.44-devpreview.4"

nuget install Microsoft.Windows.CppWinRT -Version 3.0.260520.1 -OutputDirectory $Packages -NonInteractive | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "Installing Microsoft.Windows.CppWinRT 3.0.260520.1 failed with exit code $LASTEXITCODE"
}
nuget install Windows.Devices.Midi2 -Version 0.99.44-devpreview.4 -OutputDirectory $Packages -NonInteractive | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "Installing Windows.Devices.Midi2 0.99.44-devpreview.4 failed with exit code $LASTEXITCODE"
}

$Headers = Get-ChildItem $MidiPackage -Recurse -Filter "Windows.Devices.Midi2.h" |
    ForEach-Object { Split-Path -Parent (Split-Path -Parent $_.FullName) }
$CppWinRt = Get-ChildItem $CppWinRtPackage -Recurse -Filter "base.h" |
    ForEach-Object { Split-Path -Parent (Split-Path -Parent $_.FullName) }
$IncludePath = (@($Headers) + @($CppWinRt) | Select-Object -Unique) -join ";"

if (-not $IncludePath) {
    throw "Windows MIDI SDK headers were not found"
}

Write-Output $IncludePath
