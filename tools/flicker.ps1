# Clicks through taskbar interactions while probing one pixel of the pill (the accent circle)
# every frame. Every line must say "hidden in 0". Restores the cursor afterwards.
param(
  [string]$Exe = "$PSScriptRoot/../bin/pebbly.exe",
  [string]$ExeArgs = "",
  [int]$ProbeX = 870, [int]$ProbeY = 1055, [int]$MinBlue = 150,
  [int]$EmptyX = 1250, [int]$BarY = 1055,
  [switch]$NoLaunch
)
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Diagnostics;
public static class Flick {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] static extern void keybd_event(byte k, byte s, uint f, IntPtr e);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
  public static int PX, PY, Min;
  public static void Click(int x, int y, bool right) { SetCursorPos(x, y); uint d = right ? 8u : 2u; mouse_event(d,0,0,0,IntPtr.Zero); mouse_event(d*2,0,0,0,IntPtr.Zero); }
  public static void Key(byte k) { keybd_event(k,0,0,IntPtr.Zero); keybd_event(k,0,2,IntPtr.Zero); }
  public static int Blue() { IntPtr dc = GetDC(IntPtr.Zero); uint c = GetPixel(dc, PX, PY); ReleaseDC(IntPtr.Zero, dc); return (int)((c >> 16) & 0xFF); }
  public static string Watch(int ms) {
    var sw = Stopwatch.StartNew(); int n = 0, hidden = 0; long first = -1, last = -1;
    while (sw.ElapsedMilliseconds < ms) { n++; if (Blue() < Min) { hidden++; if (first < 0) first = sw.ElapsedMilliseconds; last = sw.ElapsedMilliseconds; } }
    return String.Format("{0} samples, hidden in {1}{2}", n, hidden, hidden > 0 ? String.Format(" (from {0} ms to {1} ms)", first, last) : "");
  }
}
"@
[Flick]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
[Flick]::PX = $ProbeX; [Flick]::PY = $ProbeY; [Flick]::Min = $MinBlue
$orig = New-Object Flick+P; [Flick]::GetCursorPos([ref]$orig) | Out-Null
if (-not $NoLaunch) {
  Get-Process pebbly -ErrorAction SilentlyContinue | Stop-Process -Force
  if ($ExeArgs) { Start-Process $Exe -ArgumentList $ExeArgs | Out-Null } else { Start-Process $Exe | Out-Null }
  Start-Sleep -Milliseconds 1500
}
"baseline blue=" + [Flick]::Blue()
"idle:                     " + [Flick]::Watch(800)
[Flick]::Click($EmptyX, $BarY, $false); "click empty taskbar:      " + [Flick]::Watch(1200)
[Flick]::Click(40, $BarY, $false);      "Start button:             " + [Flick]::Watch(1200); [Flick]::Key(0x1B); "  Esc:                    " + [Flick]::Watch(700)
[Flick]::Click(96, $BarY, $false);      "Search:                   " + [Flick]::Watch(1200); [Flick]::Key(0x1B); "  Esc:                    " + [Flick]::Watch(700)
[Flick]::Click(1442, $BarY, $false);    "tray overflow:            " + [Flick]::Watch(1200); [Flick]::Key(0x1B); "  Esc:                    " + [Flick]::Watch(700)
[Flick]::Click(1825, $BarY, $false);    "clock / notifications:    " + [Flick]::Watch(1200); [Flick]::Key(0x1B); "  Esc:                    " + [Flick]::Watch(700)
[Flick]::Click($EmptyX, $BarY, $true);  "right-click taskbar menu: " + [Flick]::Watch(1200); [Flick]::Key(0x1B); "  Esc:                    " + [Flick]::Watch(700)
[Flick]::SetCursorPos(206, $BarY) | Out-Null; "hover app icon:           " + [Flick]::Watch(1500)
[Flick]::SetCursorPos($EmptyX, $BarY) | Out-Null; "hover off:                " + [Flick]::Watch(700)
[Flick]::SetCursorPos($orig.X, $orig.Y) | Out-Null
