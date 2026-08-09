param(
    [Parameter(Mandatory = $true, Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Command
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$env:VMM_WINDOWS_MIDI_SDK_INCLUDE = & (Join-Path $Root "scripts/prepare-windows-midi-sdk.ps1") -Root $Root

$Executable = $Command[0]
$Arguments = if ($Command.Count -gt 1) { $Command[1..($Command.Count - 1)] } else { @() }
& $Executable @Arguments
exit $LASTEXITCODE
