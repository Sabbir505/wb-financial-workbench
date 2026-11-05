# Capture one screenshot of wb-app: starts it with the given args, waits for
# the window, waits DelayMs more, PrintWindow-captures to $Out, then stops it.
param(
  [Parameter(Mandatory=$true)][string]$Out,
  [string]$AppArgs = "",
  [int]$DelayMs = 2500,
  [string]$Vault = "",
  [string]$Data = ""
)
$ErrorActionPreference = "Stop"
if ($Vault) { $env:WB_VAULT = $Vault }
if ($Data)  { $env:WB_DATA  = $Data }

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Cap {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -AssemblyName System.Drawing

$proc = Start-Process -FilePath ".\target\debug\wb-app.exe" -ArgumentList $AppArgs -PassThru -WorkingDirectory (Get-Location)
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 100; $i++) {
  Start-Sleep -Milliseconds 100
  try { $proc.Refresh() } catch { break }
  if ($proc.HasExited) { break }
  if ($proc.MainWindowHandle -ne 0) { $hwnd = $proc.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW ($Out)"; if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }; exit 1 }
Start-Sleep -Milliseconds $DelayMs

$r = New-Object Cap+RECT
[Cap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
$w = $r.R - $r.L; $h = $r.B - $r.T
if ($w -le 0 -or $h -le 0) { Write-Output "BAD RECT ($Out)"; Stop-Process -Id $proc.Id -Force; exit 1 }
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [Cap]::PrintWindow($hwnd, $hdc, 2)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save($Out)
$bmp.Dispose()
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
Write-Output "saved $Out ok=$ok ${w}x${h}"
