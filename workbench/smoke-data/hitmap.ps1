# Hit-test map: hover a grid over the window and record where the Approve
# button's hover color (0x82d9a6) actually appears.
param(
  [Parameter(Mandatory=$true)][string]$Out,
  [Parameter(Mandatory=$true)][string]$Vault,
  [Parameter(Mandatory=$true)][string]$Data
)
$ErrorActionPreference = "Stop"
$env:WB_VAULT = $Vault
$env:WB_DATA  = $Data

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Cap {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
  public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -AssemblyName System.Drawing

$proc = Start-Process -FilePath ".\target\debug\wb-app.exe" -PassThru -WorkingDirectory (Get-Location)
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 100; $i++) {
  Start-Sleep -Milliseconds 100
  try { $proc.Refresh() } catch { break }
  if ($proc.HasExited) { break }
  if ($proc.MainWindowHandle -ne 0) { $hwnd = $proc.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW"; exit 1 }

Start-Sleep -Milliseconds 1800
$r = New-Object Cap+RECT
[Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
[Cap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 300

# open the first row -> drawer
[Cap]::SetCursorPos($r.L + 330, $r.T + 303) | Out-Null
Start-Sleep -Milliseconds 200
[Cap]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
[Cap]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 1500

$w = $r.R - $r.L; $h = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hits = New-Object System.Collections.ArrayList
for ($x = 500; $x -le 1150; $x += 25) {
  for ($y = 150; $y -le 500; $y += 25) {
    [Cap]::SetCursorPos($r.L + $x, $r.T + $y) | Out-Null
    Start-Sleep -Milliseconds 90
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [Cap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $p = $bmp.GetPixel($x, $y)
    if ([Math]::Abs($p.R - 130) -le 18 -and [Math]::Abs($p.G - 217) -le 18 -and [Math]::Abs($p.B - 166) -le 18) {
      [void]$hits.Add("${x},${y}")
    }
  }
}
$g.Dispose(); $bmp.Dispose()
Write-Output ("hover hits: " + ($hits -join ' '))
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
Write-Output "done"
