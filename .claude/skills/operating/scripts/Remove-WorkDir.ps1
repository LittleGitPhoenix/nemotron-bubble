<#
.SYNOPSIS
	Stops the test instance and deletes the work folder.
.DESCRIPTION
	Refuses when the folder contains a link (junction or symbolic link), because deleting it recursively could
	delete the linked data, for example the user's models (several GB). The scripts never create links,
	so a link means someone added it by hand: remove it as a link first ([System.IO.Directory]::Delete).
#>
param([Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

Assert-NotUserInstall $WorkDir
if (-not (Test-Path $WorkDir)) { 'nothing to remove'; return }
$WorkDir = (Resolve-Path $WorkDir).Path
foreach ($process in Get-NemoProcesses $WorkDir) { Stop-Process -Id $process.Id; Start-Sleep -Seconds 1 }

$links = @(Get-ChildItem $WorkDir -Recurse -Force -Attributes ReparsePoint -ErrorAction SilentlyContinue)
if ($links) { throw "Not deleting $WorkDir, it contains links: $($links.FullName -join ', ')" }
Remove-Item $WorkDir -Recurse -Force
"removed $WorkDir"
