# Tall-window capture: resize wb-app beyond the screen height so more of the
# detail page is rendered and clickable without scrolling.
param(
  [Parameter(Mandatory=$true)][string]$Out,
  [Parameter(Mandatory=$true)][string]$Vault,
  [Parameter(Mandatory=$true)][string]$Data,
  [string]$Mode = "view",   # view | drawer | full | debug | library
  [string]$AppArgs = ""
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
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  public struct RECT { public int L, T, R, B; }
  public struct POINT { public int X, Y; }
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
  Write-Output "STILL BLACK $path"
}

function Click([IntPtr]$hwnd, [int]$x, [int]$y) {
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  [Cap]::SetCursorPos($r.L + $x, $r.T + $y) | Out-Null
  Start-Sleep -Milliseconds 200
  [Cap]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 40
  [Cap]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 600
}

$sp = @{ FilePath = ".\target\debug\wb-app.exe"; PassThru = $true; WorkingDirectory = (Get-Location) }
if ($AppArgs) { $sp.ArgumentList = $AppArgs -split ' ' }
$proc = Start-Process @sp
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 100; $i++) {
  Start-Sleep -Milliseconds 100
  try { $proc.Refresh() } catch { break }
  if ($proc.HasExited) { break }
  if ($proc.MainWindowHandle -ne 0) { $hwnd = $proc.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW"; exit 1 }

Start-Sleep -Milliseconds 1800
[Cap]::SetWindowPos($hwnd, [IntPtr]::Zero, 40, 0, 1454, 1170, 0x0014) | Out-Null
Start-Sleep -Milliseconds 1200
[Cap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 400

# PostMessage click: WM_LBUTTONDOWN/UP in client coordinates — lands even
# when synthetic cursor clicks are swallowed.
function Post-Click([IntPtr]$hwnd, [int]$pngX, [int]$pngY) {
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  $origin = New-Object Cap+POINT
  $origin.X = 0; $origin.Y = 0
  [Cap]::ClientToScreen($hwnd, [ref]$origin) | Out-Null
  $cx = $r.L + $pngX - $origin.X
  $cy = $r.T + $pngY - $origin.Y
  $lp = [IntPtr](($cy -shl 16) -bor ($cx -band 0xFFFF))
  [Cap]::PostMessage($hwnd, 0x0200, [IntPtr]::Zero, $lp) | Out-Null   # move
  Start-Sleep -Milliseconds 120
  [Cap]::PostMessage($hwnd, 0x0201, [IntPtr]1, $lp) | Out-Null        # down
  Start-Sleep -Milliseconds 60
  [Cap]::PostMessage($hwnd, 0x0202, [IntPtr]::Zero, $lp) | Out-Null   # up
  Start-Sleep -Milliseconds 600
}

if ($Mode -eq "pmfull") {
  Post-Click $hwnd 330 303
  Start-Sleep -Milliseconds 800
  Save-Shot $hwnd "$($Out).step-1-drawer.png"
  Post-Click $hwnd 925 1111
  Start-Sleep -Milliseconds 900
  Save-Shot $hwnd "$($Out).step-2-full.png"
}

function Test-InFullView([IntPtr]$hwnd) {
  # The solid-green Approve button only exists in the full view.
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ([int]($r.R - $r.L)), ([int]($r.B - $r.T))
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  [Cap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
  $g.ReleaseHdc($hdc); $g.Dispose()
  $p = $bmp.GetPixel(330, 224)
  $bmp.Dispose()
  return ($p.G -gt 140 -and $p.R -lt 160)
}

if ($Mode -eq "hoverprobe") {
  Click $hwnd 330 303
  Start-Sleep -Milliseconds 1500
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  [Cap]::SetCursorPos($r.L + 880, $r.T + 255) | Out-Null
  Start-Sleep -Milliseconds 800
  Save-Shot $hwnd "$($Out).hover-drawer.png"
  [Cap]::SetCursorPos($r.L + 400, $r.T + 303) | Out-Null
  Start-Sleep -Milliseconds 800
  Save-Shot $hwnd "$($Out).hover-row.png"
}

if ($Mode -eq "hover") {
  Click $hwnd 330 303   # real click: open drawer (this one works)
  Start-Sleep -Milliseconds 1500
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  [Cap]::SetCursorPos($r.L + 899, $r.T + 255) | Out-Null   # over Approve (green)
  Start-Sleep -Milliseconds 700
  Save-Shot $hwnd "$($Out).step-hover-approve.png"
  [Cap]::SetCursorPos($r.L + 925, $r.T + 1111) | Out-Null  # over Open full view
  Start-Sleep -Milliseconds 700
  Save-Shot $hwnd "$($Out).step-hover-expand.png"
}

if ($Mode -eq "accordions") {
  Click $hwnd 330 303
  Start-Sleep -Milliseconds 1500
  for ($try = 0; $try -lt 6; $try++) {
    if (Test-InFullView $hwnd) { break }
    Click $hwnd 925 1111
    Start-Sleep -Milliseconds 900
  }
  Save-Shot $hwnd "$($Out).step-1-full.png"
  foreach ($x in 105, 400) {
    Post-Click $hwnd $x 906
    Start-Sleep -Milliseconds 450
  }
  Save-Shot $hwnd "$($Out).step-2-meta.png"
  foreach ($x in 105, 400) {
    Post-Click $hwnd $x 943
    Start-Sleep -Milliseconds 450
  }
  Save-Shot $hwnd $Out
}

if ($Mode -eq "library") {
  Click $hwnd 400 296
  Start-Sleep -Milliseconds 1500
  Save-Shot $hwnd "$($Out).step-reader.png"
  Click $hwnd 1403 130
  Start-Sleep -Milliseconds 700
  Save-Shot $hwnd $Out
}

if ($Mode -eq "quad") {
  Save-Shot $hwnd $Out
  Click $hwnd 1000 262
  Start-Sleep -Milliseconds 1800
  Save-Shot $hwnd "$($Out).step-drawer.png"
}

if ($Mode -eq "view") {
  Save-Shot $hwnd $Out
} elseif ($Mode -eq "drawer") {
  Click $hwnd 330 303
  Start-Sleep -Milliseconds 800
  Save-Shot $hwnd $Out
} elseif ($Mode -eq "full") {
  Click $hwnd 330 303
  Start-Sleep -Milliseconds 800
  Click $hwnd 925 1111
  Start-Sleep -Milliseconds 900
  Save-Shot $hwnd $Out
} elseif ($Mode -eq "debug") {
  Click $hwnd 330 303
  Start-Sleep -Milliseconds 800
  Save-Shot $hwnd "$($Out).step-1-open.png"
  Click $hwnd 1403 130
  Start-Sleep -Milliseconds 800
  Save-Shot $hwnd "$($Out).step-2-after-x.png"
  Click $hwnd 330 303
  Start-Sleep -Milliseconds 800
  Click $hwnd 935 1113
  Start-Sleep -Milliseconds 900
  Save-Shot $hwnd "$($Out).step-3-after-expand.png"
}

if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
Write-Output "done $Mode"
