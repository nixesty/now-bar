# Two activities at once: starts a 5-minute timer and a stopwatch from the picker, then turns the
# mouse wheel over the pill. Saves screenshots into -Dir. No alarm.
param(
  [string]$Exe = "$PSScriptRoot/../target/quick/pebbly.exe",
  [string]$Dir = "$env:TEMP/pebbly-stack-check"
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Threading;
public static class Sc {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, int x, int y, int d, IntPtr e);
  [DllImport("user32.dll")] static extern void keybd_event(byte k, byte s, uint f, IntPtr e);
  public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(2,0,0,0,IntPtr.Zero); Thread.Sleep(80); mouse_event(4,0,0,0,IntPtr.Zero); }
  public static void Wheel(int x, int y, int notches) { SetCursorPos(x, y); for (int i = 0; i < Math.Abs(notches); i++) { mouse_event(0x0800,0,0, notches > 0 ? 120 : -120, IntPtr.Zero); Thread.Sleep(40); } }
  public static void Key(byte k) { keybd_event(k,0,0,IntPtr.Zero); keybd_event(k,0,2,IntPtr.Zero); }
  public static void WinAltN() { keybd_event(0x5B,0,0,IntPtr.Zero); keybd_event(0x12,0,0,IntPtr.Zero); keybd_event(0x4E,0,0,IntPtr.Zero); keybd_event(0x4E,0,2,IntPtr.Zero); keybd_event(0x12,0,2,IntPtr.Zero); keybd_event(0x5B,0,2,IntPtr.Zero); }
}
"@
[Sc]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
New-Item -ItemType Directory -Force $Dir | Out-Null; Get-ChildItem $Dir -Filter *.png | Remove-Item
$orig = New-Object Sc+P; [Sc]::GetCursorPos([ref]$orig) | Out-Null
$bmp = New-Object System.Drawing.Bitmap 700, 120; $g = [System.Drawing.Graphics]::FromImage($bmp)
function Shot($name) { $g.CopyFromScreen(610, 960, 0, 0, $bmp.Size); $bmp.Save("$Dir/$name.png", [System.Drawing.Imaging.ImageFormat]::Png) }
function Away { [Sc]::SetCursorPos(1600, 500) | Out-Null }

Get-Process pebbly -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process $Exe | Out-Null; Start-Sleep -Milliseconds 1500
[Sc]::WinAltN(); Start-Sleep -Milliseconds 700; [Sc]::Click(856, 734); Start-Sleep -Milliseconds 200; [Sc]::Click(1103, 880); [Sc]::Key(0x0D); Away; Start-Sleep -Milliseconds 900
Shot "1-timer"
[Sc]::WinAltN(); Start-Sleep -Milliseconds 700; [Sc]::Click(1065, 734); Start-Sleep -Milliseconds 200; [Sc]::Key(0x0D); Away; Start-Sleep -Milliseconds 900
Shot "2-timer-and-stopwatch"
[Sc]::Wheel(900, 1055, -1); Away; Start-Sleep -Milliseconds 150; Shot "3-wheel-fading"
Start-Sleep -Milliseconds 500; Shot "4-wheel-done"
[Sc]::Wheel(900, 1055, -1); Away; Start-Sleep -Milliseconds 700; Shot "5-wheel-back"
[Sc]::SetCursorPos($orig.X, $orig.Y) | Out-Null
"done: $Dir"
