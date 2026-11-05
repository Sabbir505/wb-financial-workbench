# Drive wb-app --selfcheck-e2e and capture a timed sequence, retrying black
# GPU frames. App stdout/stderr go to $Prefix-app.log for diagnosis.
param(
  [Parameter(Mandatory=$true)][string]$Prefix,
  [Parameter(Mandatory=$true)][string]$Vault,
  [Parameter(Mandatory=$true)][string]$Data,
  [int]$ModalHoldMs = 2500,
  [int]$FinalHoldMs = 15000
)
$ErrorActionPreference = "Stop"
$env:WB_VAULT = $Vault
$env:WB_DATA  = $Data
$env:WB_HOLD_MODAL_MS = $ModalHoldMs
$env:WB_HOLD_FINAL_MS = $FinalHoldMs

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Cap {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Test-Black($bmp) {
  # Sample a grid; true if effectively all-black (PrintWindow GPU race).
  for ($y = 40; $y -lt $bmp.Height; $y += 120) {
    for ($x = 40; $x -lt $bmp.Width; $x += 120) {
      $p = $bmp.GetPixel($x, $y)
      if ($p.R -gt 12 -or $p.G -gt 12 -or $p.B -gt 12) { return $false }
    }
  }
  return $true
}

function Save-Shot([IntPtr]$hwnd, [string]$path) {
  for ($try = 0; $try -lt 6; $try++) {
    $r = New-Object Cap+RECT
    [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $h = $r.B - $r.T
    if ($w -le 0 -or $h -le 0) { Write-Output "GONE $path"; return $false }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [Cap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $black = Test-Black $bmp
    if (-not $black) { $bmp.Save($path); $bmp.Dispose(); Write-Output "saved $path"; return $true }
    $bmp.Dispose()
    Start-Sleep -Milliseconds 250
  }
  # Fallback: bring to front and copy from screen.
  [Cap]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 300
  $r = New-Object Cap+RECT
  [Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $h = $r.B - $r.T
  if ($w -le 0 -or $h -le 0) { Write-Output "GONE $path"; return $false }
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $h)) | Out-Null
  $g.Dispose()
  $bmp.Save($path); $bmp.Dispose()
  Write-Output "saved(screen) $path"
  return $true
}

$proc = Start-Process -FilePath ".\target\debug\wb-app.exe" -ArgumentList "--selfcheck-e2e" -PassThru -WorkingDirectory (Get-Location) -RedirectStandardOutput "$Prefix-app.log" -RedirectStandardError "$Prefix-app.err"
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 100; $i++) {
  Start-Sleep -Milliseconds 100
  try { $proc.Refresh() } catch { break }
  if ($proc.HasExited) { break }
  if ($proc.MainWindowHandle -ne 0) { $hwnd = $proc.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW"; Get-Content "$Prefix-app.log" -ErrorAction SilentlyContinue; exit 1 }

Start-Sleep -Milliseconds 1800
[void](Save-Shot $hwnd "$Prefix-1-inbox.png")
Start-Sleep -Milliseconds 400
[void](Save-Shot $hwnd "$Prefix-2-detail.png")
Start-Sleep -Milliseconds 1000
[void](Save-Shot $hwnd "$Prefix-3-modal.png")
Start-Sleep -Milliseconds 1300
[void](Save-Shot $hwnd "$Prefix-4-modal2.png")

# Wait out the e2e submit, then land on the post-review detail and audit views.
Start-Sleep -Milliseconds ([Math]::Max(2000, $ModalHoldMs - 2000))
[void](Save-Shot $hwnd "$Prefix-5-after.png")
[Cap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 400
[System.Windows.Forms.SendKeys]::SendWait("^4")
Start-Sleep -Milliseconds 800
[void](Save-Shot $hwnd "$Prefix-6-audit.png")

if ($proc.HasExited) {
  Write-Output "APP EXITED EARLY:"
  Get-Content "$Prefix-app.log" -ErrorAction SilentlyContinue
  Get-Content "$Prefix-app.err" -ErrorAction SilentlyContinue
} else {
  Stop-Process -Id $proc.Id -Force
  Write-Output "sequence done"
}
