<#
.SYNOPSIS
	Takes a screenshot of a test instance's settings window or bubble.
.DESCRIPTION
	Default: the settings window via PrintWindow. That works when it is covered and needs no focus, but it does not
	include popup menus. Its pixels have the same origin as Invoke-Target.ps1 -X/-Y.
	-Screen: a screen capture of the settings window united with the open popup menus. The app keeps both topmost.
	-Window Bubble: the floating bubble. It is a layered window, so it is always captured from the screen.
	-Delay waits that many seconds first (animations, a running download). -Crop x,y,w,h and -Zoom n cut out and enlarge a detail.
#>
param
(
	[Parameter(Mandatory)][int]$ProcessId,
	[Parameter(Mandatory)][string]$Out,
	[ValidateSet('Settings', 'Bubble')][string]$Window = 'Settings',
	[switch]$Screen,
	[ValidateRange(0, 60)][double]$Delay = 0, # Seconds, not milliseconds.
	[int[]]$Crop,
	[double]$Zoom = 1
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\_Common.ps1"

if ($Delay -gt 0) { Start-Sleep -Milliseconds ([int]($Delay * 1000)) }
$directory = Split-Path $Out
if ($directory -and -not (Test-Path $directory)) { New-Item -ItemType Directory -Path $directory | Out-Null }

$handle = Get-NemoWindow $ProcessId $Window
if (-not [NemoNative]::IsWindowVisible($handle)) { throw "The $Window window is not visible. Open-Settings.ps1 opens the settings window." }
$rect = Get-NemoRect $handle

if ($Screen -or $Window -eq 'Bubble')
{
	$left = $rect.L; $top = $rect.T; $right = $rect.R; $bottom = $rect.B
	if ($Window -eq 'Settings')
	{
		foreach ($menu in Get-NemoMenus $ProcessId)
		{
			$m = Get-NemoRect $menu
			$left = [Math]::Min($left, $m.L); $top = [Math]::Min($top, $m.T)
			$right = [Math]::Max($right, $m.R); $bottom = [Math]::Max($bottom, $m.B)
		}
	}
	$bitmap = New-Object System.Drawing.Bitmap ($right - $left), ($bottom - $top)
	$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
	$graphics.CopyFromScreen($left, $top, 0, 0, $bitmap.Size)
	$graphics.Dispose()
	if ($left -ne $rect.L -or $top -ne $rect.T) { "note: the image origin is shifted by $($rect.L - $left),$($rect.T - $top) against window pixels" }
}
else
{
	$bitmap = New-Object System.Drawing.Bitmap ($rect.R - $rect.L), ($rect.B - $rect.T)
	$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
	$hdc = $graphics.GetHdc()
	[NemoNative]::PrintWindow($handle, $hdc, 2) | Out-Null # PW_RENDERFULLCONTENT
	$graphics.ReleaseHdc($hdc)
	$graphics.Dispose()
}

if ($Crop)
{
	$cropped = $bitmap.Clone((New-Object System.Drawing.Rectangle $Crop[0], $Crop[1], $Crop[2], $Crop[3]), $bitmap.PixelFormat)
	$bitmap.Dispose(); $bitmap = $cropped
}
if ($Zoom -ne 1)
{
	$scaled = New-Object System.Drawing.Bitmap ([int]($bitmap.Width * $Zoom)), ([int]($bitmap.Height * $Zoom))
	$graphics = [System.Drawing.Graphics]::FromImage($scaled)
	$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
	$graphics.DrawImage($bitmap, 0, 0, $scaled.Width, $scaled.Height)
	$graphics.Dispose(); $bitmap.Dispose(); $bitmap = $scaled
}
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
"$($bitmap.Width) x $($bitmap.Height) -> $Out"
$bitmap.Dispose()
