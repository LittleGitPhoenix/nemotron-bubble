<#
.SYNOPSIS
	Picks an entry of the open popup menu of a test instance (Language, Microphone, Bubble position, tray menu).
.DESCRIPTION
	Popup menus block the app until they close, and they are driven by keys: -Index n presses Down n times and Enter.
	The first Down highlights the first entry, separators are skipped. Take a screenshot with -Screen first to count.
	-Cancel closes the menu with Escape. -Tray opens the tray icon's right-click menu first.
	Submenus (in the tray menu) open with -Right after the Downs, then -SubIndex picks inside the submenu.
#>
param
(
	[Parameter(Mandatory)][int]$ProcessId,
	[int]$Index,
	[switch]$Right,
	[int]$SubIndex,
	[switch]$Cancel,
	[switch]$Tray
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

function Send-Key([IntPtr]$Menu, [int]$Key, [int]$Times = 1)
{
	for ($i = 0; $i -lt $Times; $i++)
	{
		[NemoNative]::PostMessageW($Menu, $script:WM_KEYDOWN, [IntPtr]$Key, [IntPtr]0) | Out-Null
		Start-Sleep -Milliseconds 150
	}
}

if ($Tray)
{
	$main = Get-NemoWindow $ProcessId 'Bubble'
	[NemoNative]::PostMessageW($main, $script:WM_TRAYICON, [IntPtr]0, [IntPtr]$script:WM_RBUTTONUP) | Out-Null
	Start-Sleep -Milliseconds 500
	if (-not $Index -and -not $Cancel) { 'tray menu open'; return }
}

$menus = Get-NemoMenus $ProcessId
if ($menus.Count -eq 0) { throw 'No popup menu is open. Click the button that opens it first.' }
$menu = $menus[-1]

if ($Cancel) { Send-Key $menu 0x1B; 'menu closed'; return }
Send-Key $menu 0x28 $Index # VK_DOWN
if ($Right)
{
	Send-Key $menu 0x27 # VK_RIGHT opens the submenu
	Start-Sleep -Milliseconds 300
	$menu = (Get-NemoMenus $ProcessId)[-1]
	Send-Key $menu 0x28 $SubIndex
}
Send-Key $menu 0x0D # VK_RETURN
Start-Sleep -Milliseconds 500
"picked entry $Index$(if ($Right) { " > $SubIndex" })"
