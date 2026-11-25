# Shared helpers of the operating scripts. Dot-source it: . "$PSScriptRoot\_Common.ps1"
# Windows PowerShell 5.1 compatible. All input is posted window messages, so the
# mouse never moves and the focus stays where it is.
Add-Type -AssemblyName System.Drawing, System.Windows.Forms

$script:RepoRoot = (Resolve-Path "$PSScriptRoot\..\..\..\..").Path
# Where the local cross-build script (scripts\build-wsl.ps1, not in the repository) deploys the
# user's own installation. Never start, stop or overwrite it from these scripts.
$script:UserInstallDir = Join-Path $script:RepoRoot 'target\wsl-release'
$script:WindowClass = 'NemotronBubbleWindow'
$script:BubbleTitle = 'Nemotron Bubble'
$script:SettingsTitle = 'Nemotron Bubble Settings'

if (-not ('NemoNative' -as [type]))
{
	Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class NemoNative
{
	[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
	public delegate bool EnumProc(IntPtr h, IntPtr l);
	[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
	[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
	[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
	[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
	[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
	[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
	[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
	[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);

	// Top level windows of one process, optionally of one class and title.
	public static List<IntPtr> Find(uint pid, string cls, string title)
	{
		var found = new List<IntPtr>();
		EnumWindows((h, l) =>
		{
			uint owner; GetWindowThreadProcessId(h, out owner);
			if (owner != pid) return true;
			var c = new StringBuilder(256); GetClassNameW(h, c, 256);
			var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
			if ((cls == null || c.ToString() == cls) && (title == null || t.ToString() == title)) found.Add(h);
			return true;
		}, IntPtr.Zero);
		return found;
	}
}
'@
}

$script:WM_KEYDOWN = 0x0100
$script:WM_LBUTTONDOWN = 0x0201
$script:WM_LBUTTONUP = 0x0202
$script:WM_RBUTTONUP = 0x0205
$script:WM_MOUSEWHEEL = 0x020A
$script:WM_TRAYICON = 0x0401 # WM_USER + 1, see src/ui/tray.rs

function Get-MakeLParam([int]$X, [int]$Y) { [IntPtr](($Y -shl 16) -bor ($X -band 0xFFFF)) }

# A window of the test instance: 'Bubble' (also the tray icon's message window) or 'Settings'.
# Always filtered by process id, so the user's own running instance is never touched.
function Get-NemoWindow([int]$ProcessId, [ValidateSet('Bubble', 'Settings')][string]$Window)
{
	$title = if ($Window -eq 'Settings') { $script:SettingsTitle } else { $script:BubbleTitle }
	$handle = [NemoNative]::Find([uint32]$ProcessId, $script:WindowClass, $title) | Select-Object -First 1
	if (-not $handle) { throw "No '$Window' window for process $ProcessId. Is it the PID that Start-Nemotron.ps1 printed?" }
	$handle
}

# Open popup menus (class #32768) of the test instance.
function Get-NemoMenus([int]$ProcessId)
{
	@([NemoNative]::Find([uint32]$ProcessId, '#32768', $null) | Where-Object { [NemoNative]::IsWindowVisible($_) })
}

function Get-NemoRect([IntPtr]$Handle)
{
	$rect = New-Object NemoNative+RECT
	[NemoNative]::GetWindowRect($Handle, [ref]$rect) | Out-Null
	$rect
}

# A click: button down and up at window coordinates (the windows have no frame,
# so window and client coordinates are the same). A button-up without a
# preceding drag is what the app treats as a click.
function Send-NemoClick([IntPtr]$Handle, [int]$X, [int]$Y)
{
	[NemoNative]::PostMessageW($Handle, $script:WM_LBUTTONDOWN, [IntPtr]1, (Get-MakeLParam $X $Y)) | Out-Null
	[NemoNative]::PostMessageW($Handle, $script:WM_LBUTTONUP, [IntPtr]0, (Get-MakeLParam $X $Y)) | Out-Null
	Start-Sleep -Milliseconds 400
}

# Mouse wheel: positive notches scroll down (120 per notch, as Windows sends it).
function Send-NemoWheel([IntPtr]$Handle, [int]$Notches)
{
	$delta = -120 * [Math]::Sign($Notches)
	for ($i = 0; $i -lt [Math]::Abs($Notches); $i++)
	{
		[NemoNative]::PostMessageW($Handle, $script:WM_MOUSEWHEEL, [IntPtr]([int]($delta -shl 16)), [IntPtr]0) | Out-Null
		Start-Sleep -Milliseconds 150
	}
	Start-Sleep -Milliseconds 250
}

# Processes started from a work folder (the test instances), never the user's installation.
function Get-NemoProcesses([string]$WorkDir)
{
	$dir = (Resolve-Path $WorkDir -ErrorAction SilentlyContinue).Path
	if (-not $dir) { return @() }
	@(Get-Process nemotron-bubble -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($dir + '\', [StringComparison]::OrdinalIgnoreCase) })
}

function Assert-NotUserInstall([string]$WorkDir)
{
	$full = [System.IO.Path]::GetFullPath($WorkDir).TrimEnd('\')
	$repo = $script:RepoRoot.TrimEnd('\')
	if ($full -ieq $script:UserInstallDir -or $full -ieq $repo -or $repo.StartsWith($full + '\', [StringComparison]::OrdinalIgnoreCase) -or $full.StartsWith($script:UserInstallDir + '\', [StringComparison]::OrdinalIgnoreCase))
	{
		throw "Refusing to use '$WorkDir': it is the user's installation or contains the repository. Use a scratch folder."
	}
}
