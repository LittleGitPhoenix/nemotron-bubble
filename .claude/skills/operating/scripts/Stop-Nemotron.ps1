<#
.SYNOPSIS
	Stops the test instance(s) started from a work folder. Never stops the user's own instance.
#>
param([Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

Assert-NotUserInstall $WorkDir
$processes = Get-NemoProcesses $WorkDir
foreach ($process in $processes) { Stop-Process -Id $process.Id; "stopped PID $($process.Id)" }
if (-not $processes) { 'no test instance running' }
Start-Sleep -Seconds 1
