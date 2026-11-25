<#
.SYNOPSIS
	Opens the settings window of a test instance (or closes it with -Close). Does nothing if it is already in that state.
.DESCRIPTION
	Uses the tray icon's left-click message, so it works with the floating bubble hidden and regardless of
	"Bubble click opens settings". The window then opens next to the mouse cursor, as after a tray click.
	-Bubble clicks the bubble instead (tests the bubble path, the window opens next to the bubble).
#>
param([Parameter(Mandatory)][int]$ProcessId, [switch]$Bubble, [switch]$Close)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

$main = Get-NemoWindow $ProcessId 'Bubble'
$settings = Get-NemoWindow $ProcessId 'Settings'
$visible = [NemoNative]::IsWindowVisible($settings)
if ($visible -ne [bool]$Close) { "already $(if ($Close) { 'closed' } else { 'open' })"; return }

if ($Bubble)
{
	$rect = Get-NemoRect $main
	Send-NemoClick $main ([int](($rect.R - $rect.L) / 2)) ([int](($rect.B - $rect.T) / 2))
}
else
{
	[NemoNative]::PostMessageW($main, $script:WM_TRAYICON, [IntPtr]0, [IntPtr]$script:WM_LBUTTONUP) | Out-Null
}
Start-Sleep -Milliseconds 500
if ([NemoNative]::IsWindowVisible($settings) -eq [bool]$Close) { throw 'The settings window did not change its state.' }
if ($Close) { 'closed' } else { 'opened' }
