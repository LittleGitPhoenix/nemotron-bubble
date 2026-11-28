<#
.SYNOPSIS
	Clicks a named element of the application window of a test instance (see Targets.ps1, -List prints all names).
.DESCRIPTION
	Opens the application window if needed (via the tray icon message, so it works with the bubble hidden), switches to
	the element's tab and, on the Settings tab, scrolls so the element is visible. Then it posts the click.
	No mouse movement, no focus change by the script. Note that the app itself brings its application window and popup
	menus to the front.
	-X/-Y click raw window pixels instead (same origin as Get-Screenshot.ps1), for elements without a name such as
	further copy icons in the History list. -Tab switches to a tab before such a raw click.
#>
param
(
	[Parameter(Mandatory, ParameterSetName = 'Name')][string]$Target,
	[Parameter(Mandatory, ParameterSetName = 'Raw')][int]$X,
	[Parameter(Mandatory, ParameterSetName = 'Raw')][int]$Y,
	[Parameter(ParameterSetName = 'Raw')][ValidateSet('History', 'Model', 'Settings')][string]$Tab,
	[Parameter(Mandatory, ParameterSetName = 'List')][switch]$List,
	[Parameter(ParameterSetName = 'Name')][Parameter(ParameterSetName = 'Raw')][int]$ProcessId
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"
. "$PSScriptRoot\Targets.ps1"

if ($List)
{
	$script:Targets.GetEnumerator() | ForEach-Object { '{0,-28} {1,-9} {2},{3}' -f $_.Key, $_.Value.Tab, $_.Value.X, $_.Value.Y }
	return
}
if (-not $ProcessId) { throw '-ProcessId is required.' }

& "$PSScriptRoot\Open-Window.ps1" -ProcessId $ProcessId | Out-Null
$window = Get-NemoWindow $ProcessId 'Application'

# -Tab is validated, so the target's tab (empty for frame targets) goes into its own variable.
$clickTab = $Tab
$clickX = $X
$clickY = $Y
if ($Target)
{
	if (-not $script:Targets.Contains($Target)) { throw "Unknown target '$Target'. Use -List." }
	$entry = $script:Targets[$Target]
	$clickTab = $entry.Tab
	$clickX = $entry.X
	$clickY = $entry.Y
}
if ($clickTab)
{
	$tabTarget = $script:Targets["Tab:$clickTab"]
	Send-NemoClick $window $tabTarget.X $tabTarget.Y
}
if ($Target -and $clickTab -eq 'Settings')
{
	# Scroll to a known offset: the top, or the bottom for rows below the visible card.
	if ($clickY -lt $script:SettingsViewBottom - 20) { Send-NemoWheel $window -4 }
	else { Send-NemoWheel $window 4; $clickY -= $script:SettingsTabMaxScroll }
}
Send-NemoClick $window $clickX $clickY
$X = $clickX
$Y = $clickY
"clicked $(if ($Target) { $Target } else { 'raw' }) at $X,$Y"
