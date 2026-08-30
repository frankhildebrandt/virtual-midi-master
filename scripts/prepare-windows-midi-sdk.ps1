param(
    [string]$Root = (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
)

$ErrorActionPreference = "Stop"
$Packages = Join-Path $Root ".packages"
$MidiFeed = Join-Path $Packages "midi-feed"
$Projection = Join-Path $Packages "midi-projection"
$CppWinRtVersion = "3.0.260520.1"
$CppWinRtPackage = Join-Path $Packages "Microsoft.Windows.CppWinRT.$CppWinRtVersion"
$MidiPackageId = "Microsoft.Windows.Devices.Midi2"
$MidiPackageVersion = "1.0.17-rc.4.25"
$MidiPackage = Join-Path $Packages "$MidiPackageId.$MidiPackageVersion"
$MidiPackageUrl = "https://github.com/microsoft/MIDI/releases/download/rc-4/$MidiPackageId.$MidiPackageVersion.nupkg"
$CppWinRtUrl = "https://www.nuget.org/api/v2/package/Microsoft.Windows.CppWinRT/$CppWinRtVersion"

function Install-Nupkg {
    param(
        [string]$Url,
        [string]$Archive,
        [string]$Destination
    )
    New-Item -ItemType Directory -Force (Split-Path -Parent $Archive) | Out-Null
    if (-not (Test-Path $Archive)) {
        Invoke-WebRequest -Uri $Url -OutFile $Archive
    }
    $Marker = Get-ChildItem $Destination -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $Marker) {
        if (Test-Path $Destination) {
            Remove-Item -LiteralPath $Destination -Recurse -Force
        }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [System.IO.Compression.ZipFile]::ExtractToDirectory($Archive, $Destination)
    }
}

New-Item -ItemType Directory -Force $Packages | Out-Null
Install-Nupkg -Url $CppWinRtUrl -Archive (Join-Path $MidiFeed "Microsoft.Windows.CppWinRT.$CppWinRtVersion.nupkg") -Destination $CppWinRtPackage
Install-Nupkg -Url $MidiPackageUrl -Archive (Join-Path $MidiFeed "$MidiPackageId.$MidiPackageVersion.nupkg") -Destination $MidiPackage

$MidiMetadata = Get-ChildItem $MidiPackage -Recurse -Filter "Microsoft.Windows.Devices.Midi2.winmd" |
    Select-Object -First 1 -ExpandProperty FullName
$InitHeader = Get-ChildItem $MidiPackage -Recurse -Filter "Microsoft.Windows.Devices.Midi2.Initialization.hpp" |
    Select-Object -First 1 -ExpandProperty FullName
$CppWinRtTool = Get-ChildItem $CppWinRtPackage -Recurse -Filter "cppwinrt.exe" |
    Select-Object -First 1 -ExpandProperty FullName
$WindowsMetadataRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\UnionMetadata"
$WindowsMetadata = Get-ChildItem $WindowsMetadataRoot -Recurse -Filter "Windows.winmd" |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $MidiMetadata -or -not $InitHeader -or -not $CppWinRtTool -or -not $WindowsMetadata) {
    throw "Windows MIDI SDK metadata, initialization header, system metadata, or C++/WinRT tooling was not found"
}

& $CppWinRtTool -input $WindowsMetadata -input $MidiMetadata -output $Projection
if ($LASTEXITCODE -ne 0) {
    throw "Generating Windows MIDI SDK C++ projections failed with exit code $LASTEXITCODE"
}

$Headers = Join-Path $Projection "winrt"
if (-not (Test-Path (Join-Path $Headers "Microsoft.Windows.Devices.Midi2.h")) -or
    -not (Test-Path (Join-Path $Headers "base.h"))) {
    throw "Generated Windows MIDI SDK headers were not found"
}

$InitInclude = Split-Path -Parent $InitHeader
Write-Output "$Projection;$InitInclude"
