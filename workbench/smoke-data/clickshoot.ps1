# Open wb-app normally, click the first inbox row, capture the waiting-review
# detail (guardrail banner + review buttons).
param(
  [Parameter(Mandatory=$true)][string]$Out,
  [Parameter(Mandatory=$true)][string]$Vault,
  [Parameter(Mandatory=$true)][string]$Data,
  [string]$Target = "row"   # "row" = first inbox row, "help" = topbar ? button
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
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
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
  # Fallback: bring to front and copy from the screen DC.
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

$proc = Start-Process -FilePath ".\target\debug\wb-app.exe" -PassThru -WorkingDirectory (Get-Location)
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 100; $i++) {
  Start-Sleep -Milliseconds 100
  try { $proc.Refresh() } catch { break }
  if ($proc.HasExited) { break }
  if ($proc.MainWindowHandle -ne 0) { $hwnd = $proc.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW"; exit 1 }

Start-Sleep -Milliseconds 2000   # queue loaded
[Cap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 300
$r = New-Object Cap+RECT
[Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
# Click target: first inbox row (~y=267) or the topbar ? shortcut button
# (far right of the 44px topbar, after the flex spacer).
if ($Target -eq "help") { $cx0 = 1410; $cy0 = 64 } else { $cx0 = 330; $cy0 = 303 }
[Cap]::SetCursorPos($r.L + $cx0, $r.T + $cy0) | Out-Null
Start-Sleep -Milliseconds 200
[Cap]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)  # left down
[Cap]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)  # left up
Start-Sleep -Milliseconds 1200
Save-Shot $hwnd "$Out.t1.png"
Start-Sleep -Milliseconds 900
Save-Shot $hwnd "$Out.t2.png"
if ($proc.HasExited) {
  Write-Output "APP EXITED:"
  Get-Process wb-app -ErrorAction SilentlyContinue | Out-Null
} else {
  Stop-Process -Id $proc.Id -Force
}
