<#
.SYNOPSIS
	Scrolls the active tab of the application window (History list or Settings tab) by mouse-wheel notches.
	Positive -Notches scroll down, negative up. One notch is 60 px (HISTORY_WHEEL_STEP).
#>
param([Parameter(Mandatory)][int]$ProcessId, [Parameter(Mandatory)][int]$Notches)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

Send-NemoWheel (Get-NemoWindow $ProcessId 'Application') $Notches
"scrolled $Notches notch(es)"
