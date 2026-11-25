<#
.SYNOPSIS
	Starts an isolated test instance of Nemotron Bubble in a work folder and prints "PID <n>".
.DESCRIPTION
	The work folder gets its own exe, config\ and log\. The user's own installation is never touched.
	-Build      builds and copies the exe into the work folder first, which works while the user's app is running.
	            Uses the local cross-build script scripts\build-wsl.ps1 when it exists (not part of the repository),
	            otherwise "cargo build --release" and copies target\release\nemotron-bubble.exe (and DirectML.dll).
	-Settings   JSON merged into config\settings.json, e.g. '{"model":"multilingual","language":"de"}'.
	            "preload_model" defaults to false. "start_with_windows" is always forced to false, and the autostart
	            entry is restored afterwards, so a test never changes the user's autostart.
	-UseModels  lets the test copy find already downloaded models: it starts with the working directory set to the
	            folder that holds them (the app also searches models\ there). Nothing is linked or copied.
	            Never download or update models in such a test, that would write into the user's models.
	-ModelsDir  the models folder to use. Default: the first existing of target\wsl-release\models and models\.
	-NoStart    prepares the folder without starting (e.g. for --self-test).
	A running instance from the same work folder is stopped first.
#>
param
(
	[Parameter(Mandatory)][string]$WorkDir,
	[switch]$Build,
	[string]$Settings,
	[switch]$UseModels,
	[string]$ModelsDir,
	[switch]$NoStart
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

Assert-NotUserInstall $WorkDir
if (-not (Test-Path $WorkDir)) { New-Item -ItemType Directory -Path $WorkDir | Out-Null }
$WorkDir = (Resolve-Path $WorkDir).Path

foreach ($process in Get-NemoProcesses $WorkDir) { Stop-Process -Id $process.Id; Start-Sleep -Seconds 1 }

if ($Build)
{
	# cargo reports progress on stderr. In the same PowerShell, redirecting that turns it into
	# errors that stop build-wsl.ps1, so it runs in its own process and is judged by its output.
	$ErrorActionPreference = 'Continue'
	$buildScript = Join-Path $script:RepoRoot 'scripts\build-wsl.ps1'
	if (Test-Path $buildScript)
	{
		$output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $buildScript -OutputDir $WorkDir 2>&1 | ForEach-Object { "$_" }
	}
	else
	{
		if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'No Rust toolchain (cargo) found and no scripts\build-wsl.ps1. Build the app as the README describes.' }
		Push-Location $script:RepoRoot
		$output = & cargo build --release 2>&1 | ForEach-Object { "$_" }
		Pop-Location
		if (-not ($output -match '^\s*error'))
		{
			foreach ($file in 'nemotron-bubble.exe', 'DirectML.dll')
			{
				$built = Join-Path $script:RepoRoot "target\release\$file"
				if (Test-Path $built) { Copy-Item $built $WorkDir -Force }
			}
		}
	}
	$ErrorActionPreference = 'Stop'
	$output | Where-Object { $_ -match '^\s*(warning|error)|-->|Finished|Done' }
	if ($output -match '^\s*error') { throw 'Build failed.' }
}
$exe = Join-Path $WorkDir 'nemotron-bubble.exe'
if (-not (Test-Path $exe)) { throw "No exe in $WorkDir. Run with -Build." }

# config\settings.json: existing values, then -Settings, then the safety overrides.
$configDir = Join-Path $WorkDir 'config'
if (-not (Test-Path $configDir)) { New-Item -ItemType Directory -Path $configDir | Out-Null }
$settingsPath = Join-Path $configDir 'settings.json'
$values = [ordered]@{ preload_model = $false }
if (Test-Path $settingsPath)
{
	(Get-Content $settingsPath -Raw | ConvertFrom-Json).PSObject.Properties | ForEach-Object { $values[$_.Name] = $_.Value }
}
if ($Settings)
{
	($Settings | ConvertFrom-Json).PSObject.Properties | ForEach-Object { $values[$_.Name] = $_.Value }
}
$values['start_with_windows'] = $false
# UTF-8 WITHOUT a byte order mark: the app rejects a BOM and then falls back to its defaults,
# which include start_with_windows = true (Set-Content -Encoding UTF8 writes a BOM in PowerShell 5.1).
[System.IO.File]::WriteAllText($settingsPath, ($values | ConvertTo-Json), (New-Object System.Text.UTF8Encoding $false))

# Models: the app also searches models\ in its working directory (find_model_dir), so the test
# copy simply starts in the folder that holds the user's models. Nothing is linked or copied.
$workingDirectory = $WorkDir
if ($UseModels)
{
	if (-not $ModelsDir)
	{
		$ModelsDir = @((Join-Path $script:UserInstallDir 'models'), (Join-Path $script:RepoRoot 'models')) |
			Where-Object { Test-Path $_ } | Select-Object -First 1
		if (-not $ModelsDir) { throw 'No downloaded models found. Pass -ModelsDir, or test without -UseModels.' }
	}
	$ModelsDir = (Resolve-Path $ModelsDir).Path
	if ((Split-Path $ModelsDir -Leaf) -ne 'models') { throw "-ModelsDir must be a folder named 'models' (it holds nemotron\ and nemotron_multi\)." }
	$workingDirectory = Split-Path $ModelsDir
	"using the models in $ModelsDir (working directory $workingDirectory)"
}

if ($NoStart) { "prepared $WorkDir. Run the exe with the working directory $workingDirectory."; return }

# The app writes its autostart entry at every start. Remember it, so a test copy that
# ignored its settings is caught before it can do more.
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$autostartBefore = (Get-ItemProperty $runKey -Name NemotronBubble -ErrorAction SilentlyContinue).NemotronBubble
$logPath = Join-Path $WorkDir 'log\nemotron-bubble.log'
$logLinesBefore = if (Test-Path $logPath) { @(Get-Content $logPath).Count } else { 0 }

$process = Start-Process $exe -WorkingDirectory $workingDirectory -PassThru
for ($i = 0; $i -lt 50; $i++)
{
	Start-Sleep -Milliseconds 200
	if ([NemoNative]::Find([uint32]$process.Id, $script:WindowClass, $script:BubbleTitle).Count -gt 0) { break }
}
if ($process.HasExited) { throw "The test instance exited (code $($process.ExitCode)). See $logPath." }

# A test copy runs with start_with_windows = false, which REMOVES the entry. Put the user's
# entry back (or remove one the test copy created), so tests never change the autostart.
$autostartAfter = (Get-ItemProperty $runKey -Name NemotronBubble -ErrorAction SilentlyContinue).NemotronBubble
if ($autostartAfter -ne $autostartBefore)
{
	if ($null -eq $autostartBefore) { Remove-ItemProperty $runKey -Name NemotronBubble }
	else { Set-ItemProperty $runKey -Name NemotronBubble -Value $autostartBefore }
	"autostart entry restored to: $(if ($null -eq $autostartBefore) { '(none)' } else { $autostartBefore })"
}

$invalid = @(Get-Content $logPath -ErrorAction SilentlyContinue) | Select-Object -Skip $logLinesBefore | Where-Object { $_ -match 'settings\.json is invalid' }
if ($invalid)
{
	Stop-Process -Id $process.Id
	throw "Stopped the test instance: it ignored its settings.json and ran with the defaults. Log: $invalid"
}
"PID $($process.Id)"
