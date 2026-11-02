# Captures a screen region every -EveryMs for -Seconds into numbered PNGs (in-process, so the
# timing is real). Example: pwsh tools/burst.ps1 -Dir C:/tmp/burst -X 610 -Y 520 -W 700 -H 560
param([Parameter(Mandatory)][string]$Dir, [int]$X = 610, [int]$Y = 520, [int]$W = 700, [int]$H = 560, [int]$EveryMs = 150, [double]$Seconds = 4, [string]$Launch = "", [string]$LaunchArgs = "")
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class BurstDpi { [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v); }
"@
[BurstDpi]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force $Dir | Out-Null
Get-ChildItem $Dir -Filter *.png | Remove-Item
if ($Launch) {
  Get-Process pebbly -ErrorAction SilentlyContinue | Stop-Process -Force
  if ($LaunchArgs) { Start-Process $Launch -ArgumentList $LaunchArgs | Out-Null } else { Start-Process $Launch | Out-Null }
}
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$sw = [Diagnostics.Stopwatch]::StartNew()
$i = 0
while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
  $g.CopyFromScreen($X, $Y, 0, 0, $bmp.Size)
  $bmp.Save(("{0}/{1:D2}-{2:D4}ms.png" -f $Dir, $i, $sw.ElapsedMilliseconds), [System.Drawing.Imaging.ImageFormat]::Png)
  $i++
  $next = $i * $EveryMs
  $wait = $next - $sw.ElapsedMilliseconds
  if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
}
"$i frames in $Dir"
