# Drive the full detail flow: row click -> drawer -> scroll -> Open full view
# -> scroll detail -> open both accordions, capturing each step.
param(
  [Parameter(Mandatory=$true)][string]$Prefix,
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

function Save-Shot([IntPtr]$hwnd, [string]$path) {
  for ($try = 0; $try -lt 6; $try++) {
    $r = New-Object Cap+RECT
    [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $h = $r.B - $r.T
    if ($w -le 0 -or $h -le 0) { Write-Output "GONE $path"; return }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [Cap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $black = $true
    for ($y = 40; $y -lt $bmp.Height; $y += 120) {
      for ($x = 40; $x -lt $bmp.Width; $x += 120) {
        $p = $bmp.GetPixel($x, $y)
        if ($p.R -gt 12 -or $p.G -gt 12 -or $p.B -gt 12) { $black = $false; break }
      }
      if (-not $black) { break }
    }
    if (-not $black) { $bmp.Save($path); $bmp.Dispose(); Write-Output "saved $path"; return }
    $bmp.Dispose()
    Start-Sleep -Milliseconds 250
  }
  [Cap]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 400
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $h = $r.B - $r.T
  if ($w -le 0 -or $h -le 0) { Write-Output "GONE $path"; return }
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $h)) | Out-Null
  $g.Dispose()
  $bmp.Save($path); $bmp.Dispose()
  Write-Output "saved(screen) $path"
}

function Wheel([int]$x, [int]$y, [int]$notches) {
  [Cap]::SetCursorPos($x, $y) | Out-Null
  Start-Sleep -Milliseconds 150
  for ($i = 0; $i -lt $notches; $i++) {
    [Cap]::mouse_event(0x0800, 0, 0, -120, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
  }
  Start-Sleep -Milliseconds 400
}

function Click([int]$x, [int]$y) {
  [Cap]::SetCursorPos($x, $y) | Out-Null
  Start-Sleep -Milliseconds 150
  [Cap]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
  [Cap]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 500
}

$proc = Start-Process -FilePath ".\target\debug\wb-app.exe" -PassThru -WorkingDirectory (Get-Location)
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 100; $i++) {
  Start-Sleep -Milliseconds 100
  try { $proc.Refresh() } catch { break }
  if ($proc.HasExited) { break }
  if ($proc.MainWindowHandle -ne 0) { $hwnd = $proc.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW"; exit 1 }

Start-Sleep -Milliseconds 2000
[Cap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 300
$r = New-Object Cap+RECT
[Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null

# 1. open the first row -> drawer
Click ($r.L + 330) ($r.T + 303)
Start-Sleep -Milliseconds 800
Save-Shot $hwnd "$Prefix-1-drawer.png"

# 2. scroll the drawer to the bottom, then Open full view
Wheel ($r.L + 1150) ($r.T + 500) 10
Click ($r.L + 930) ($r.T + 855)
Start-Sleep -Milliseconds 900
Save-Shot $hwnd "$Prefix-2-fullview.png"

# 3. scroll the detail page to the bottom (accordions live there)
Wheel ($r.L + 700) ($r.T + 500) 14
Start-Sleep -Milliseconds 500
Save-Shot $hwnd "$Prefix-3-bottom.png"

if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
Write-Output "sequence done"
