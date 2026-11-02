# End-to-end timer/stopwatch scenario with synthetic input; saves a screenshot per step into -Dir.
# Coordinates fit 1920x1080 at 125 % with a left-aligned 50 px taskbar. Plays the alarm once.
param(
  [string]$Exe = "$PSScriptRoot/../target/quick/pebbly.exe",
  [string]$Dir = "$env:TEMP/pebbly-timer-check"
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Threading;
public static class Tc {
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
[Tc]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
New-Item -ItemType Directory -Force $Dir | Out-Null; Get-ChildItem $Dir -Filter *.png | Remove-Item
$orig = New-Object Tc+P; [Tc]::GetCursorPos([ref]$orig) | Out-Null
$bmp = New-Object System.Drawing.Bitmap 700, 420; $g = [System.Drawing.Graphics]::FromImage($bmp)
function Shot($name) { $g.CopyFromScreen(610, 660, 0, 0, $bmp.Size); $bmp.Save("$Dir/$name.png", [System.Drawing.Imaging.ImageFormat]::Png) }
function Wait($ms) { Start-Sleep -Milliseconds $ms }

Get-Process pebbly -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process $Exe | Out-Null; Wait 1500; Shot "01-nothing"
[Tc]::WinAltN(); Wait 700; Shot "02-picker"
[Tc]::Click(818, 880); Wait 300; Shot "03-preset-1min"
[Tc]::Click(960, 964); Wait 900; Shot "04-pill"
Wait 2000; Shot "05-ticking"
[Tc]::Click(900, 1055); Wait 700; Shot "06-card"
[Tc]::Click(1147, 967); Wait 500; Shot "07-paused"
[Tc]::Click(1147, 967); [Tc]::SetCursorPos(1600, 500) | Out-Null; Wait 1500; Shot "08-closed"
[Tc]::WinAltN(); Wait 700
[Tc]::Wheel(960, 807, -1); [Tc]::Wheel(1088, 807, 3); Wait 300; Shot "09-picker-3s"
[Tc]::Key(0x0D); [Tc]::SetCursorPos(1600, 500) | Out-Null; Wait 4500; Shot "10-done"
[Tc]::Click(1134, 967); [Tc]::SetCursorPos(1600, 500) | Out-Null; Wait 900; Shot "11-after-close"
[Tc]::WinAltN(); Wait 700; [Tc]::Click(1065, 734); Wait 300; Shot "12-stopwatch-tab"
[Tc]::Key(0x0D); [Tc]::SetCursorPos(1600, 500) | Out-Null; Wait 1500; Shot "13-stopwatch"
[Tc]::WinAltN(); Wait 700; [Tc]::Click(856, 734); Wait 200; [Tc]::Click(1103, 880); [Tc]::Key(0x0D); [Tc]::SetCursorPos(1600, 500) | Out-Null; Wait 900; Shot "14-two"
[Tc]::Wheel(900, 1055, -1); [Tc]::SetCursorPos(1600, 500) | Out-Null; Wait 700; Shot "15-wheel"
[Tc]::SetCursorPos($orig.X, $orig.Y) | Out-Null
"done: $Dir"
