<#
.SYNOPSIS
	Cuts a region out of an existing image (for example an annotated screenshot of the user) and optionally enlarges it.
.DESCRIPTION
	Large screenshots are scaled down when they are viewed, which hides details like marker numbers or small icons.
	Run it with -Info first to get the image size, then crop the marked regions and view the crops.
.PARAMETER Crop
	x,y,width,height in image pixels.
#>
param([Parameter(Mandatory)][string]$Path, [string]$Out, [int[]]$Crop, [double]$Zoom = 1, [switch]$Info)
Add-Type -AssemblyName System.Drawing

$bitmap = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Path).Path)
if ($Info) { "$($bitmap.Width) x $($bitmap.Height)"; $bitmap.Dispose(); return }
if (-not $Out) { throw 'Specify -Out or -Info.' }

$source = $bitmap
if ($Crop)
{
	$rectangle = New-Object System.Drawing.Rectangle $Crop[0], $Crop[1], $Crop[2], $Crop[3]
	$source = $bitmap.Clone($rectangle, $bitmap.PixelFormat)
}
$target = New-Object System.Drawing.Bitmap ([int]($source.Width * $Zoom)), ([int]($source.Height * $Zoom))
$graphics = [System.Drawing.Graphics]::FromImage($target)
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.DrawImage($source, 0, 0, $target.Width, $target.Height)
$graphics.Dispose()

$directory = Split-Path $Out
if ($directory -and -not (Test-Path $directory)) { New-Item -ItemType Directory -Path $directory | Out-Null }
$target.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
"$($target.Width) x $($target.Height) -> $Out"
$target.Dispose(); if ($source -ne $bitmap) { $source.Dispose() }; $bitmap.Dispose()
