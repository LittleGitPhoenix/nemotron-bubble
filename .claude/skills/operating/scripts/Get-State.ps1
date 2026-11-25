<#
.SYNOPSIS
	Shows what a test instance wrote: config\settings.json, the end of the log and the model folders.
.DESCRIPTION
	-Log n          number of log lines (default 15). -Grep <regex> filters them.
	-WaitFor <regex> waits until a new log line matches (e.g. 'Nemotron loaded', 'model downloaded'), up to -Timeout seconds.
	-Models          lists models\ with sizes, partial downloads and the version from model-info.json.
#>
param
(
	[Parameter(Mandatory)][string]$WorkDir,
	[int]$Log = 15,
	[string]$Grep,
	[string]$WaitFor,
	[int]$Timeout = 60,
	[switch]$Models
)
$ErrorActionPreference = 'Stop'
$logPath = Join-Path $WorkDir 'log\nemotron-bubble.log'

if ($WaitFor)
{
	$start = if (Test-Path $logPath) { @(Get-Content $logPath).Count } else { 0 }
	$deadline = (Get-Date).AddSeconds($Timeout)
	while ((Get-Date) -lt $deadline)
	{
		$hit = @(Get-Content $logPath -ErrorAction SilentlyContinue) | Select-Object -Skip $start | Where-Object { $_ -match $WaitFor } | Select-Object -First 1
		if ($hit) { "matched: $hit"; break }
		Start-Sleep -Milliseconds 500
	}
	if (-not $hit) { "timeout after $Timeout s waiting for: $WaitFor" }
}

'--- config\settings.json'
Get-Content (Join-Path $WorkDir 'config\settings.json') -ErrorAction SilentlyContinue

"--- log (last $Log$(if ($Grep) { ", matching $Grep" }))"
$lines = @(Get-Content $logPath -ErrorAction SilentlyContinue)
if ($Grep) { $lines = $lines | Where-Object { $_ -match $Grep } }
$lines | Select-Object -Last $Log

if ($Models)
{
	'--- models'
	foreach ($dir in Get-ChildItem (Join-Path $WorkDir 'models') -Directory -ErrorAction SilentlyContinue)
	{
		$info = Join-Path $dir.FullName 'model-info.json'
		$version = if (Test-Path $info) { (Get-Content $info -Raw | ConvertFrom-Json).date } else { $null }
		$files = Get-ChildItem $dir.FullName -File
		$size = ($files | Where-Object { $_.Extension -ne '.part' } | Measure-Object Length -Sum).Sum
		$partial = ($files | Where-Object { $_.Extension -eq '.part' } | Measure-Object Length -Sum).Sum
		'{0,-28} {1,10:N0} bytes  version {2}{3}' -f $dir.Name, $size, $(if ($version) { $version } else { 'unknown' }), $(if ($partial) { "  partial $partial bytes" })
	}
}
