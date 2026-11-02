# Drives the pill/card with synthetic clicks and mouse moves and reports, after each step,
# whether the card and the pill are drawn (probing one accent-coloured pixel of each).
# Defaults fit a 1920x1080 screen at 125 % with the demo timer card. Restores the cursor.
param(
  [string]$Exe = "$PSScriptRoot/../target/quick/pebbly.exe",
  [int]$PillX = 960, [int]$PillY = 1055, [int]$PillProbeX = 870,
  [int]$CardProbeX = 777, [int]$CardProbeY = 967, [int]$EmptyX = 1250
)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class CardCheck {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
  // Held for 80 ms like a real click.
  public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(2,0,0,0,IntPtr.Zero); System.Threading.Thread.Sleep(80); mouse_event(4,0,0,0,IntPtr.Zero); }
  // True when the pixel is the timer accent #5D57D2 (the opaque icon circle), so other blue
  // things on screen can't fool the check.
  public static bool Blue(int x, int y) {
    IntPtr dc = GetDC(IntPtr.Zero); uint c = GetPixel(dc, x, y); ReleaseDC(IntPtr.Zero, dc);
    int r = (int)(c & 0xFF), g = (int)((c >> 8) & 0xFF), b = (int)((c >> 16) & 0xFF);
    return Math.Abs(r - 0x5D) + Math.Abs(g - 0x57) + Math.Abs(b - 0xD2) < 24;
  }
}
"@
[CardCheck]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
$orig = New-Object CardCheck+P; [CardCheck]::GetCursorPos([ref]$orig) | Out-Null
function State($label) { "{0,-28} card={1,-5} pill={2}" -f $label, [CardCheck]::Blue($CardProbeX, $CardProbeY), [CardCheck]::Blue($PillProbeX, $PillY) }
function MoveTo($x, $y, $ms) { [CardCheck]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds $ms }

Get-Process pebbly -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process $Exe | Out-Null; Start-Sleep -Milliseconds 1500
State "start"
[CardCheck]::Click($PillX, $PillY); Start-Sleep -Milliseconds 700; State "click pill"
[CardCheck]::Click($EmptyX, $PillY); Start-Sleep -Milliseconds 700; State "click empty taskbar"
Start-Sleep -Milliseconds 1500; State "  1.5 s later"
[CardCheck]::Click($PillX, $PillY); Start-Sleep -Milliseconds 700; State "click pill again"
MoveTo ($CardProbeX + 100) $CardProbeY 200; MoveTo ($CardProbeX + 100) 600 300; State "mouse left card, 0.3 s"
Start-Sleep -Milliseconds 800; State "mouse left card, 1.1 s"
[CardCheck]::Click($PillX, $PillY); Start-Sleep -Milliseconds 700
MoveTo ($CardProbeX + 100) $CardProbeY 200; MoveTo ($CardProbeX + 100) 600 300; MoveTo ($CardProbeX + 100) $CardProbeY 900
State "left the card and came back"
[CardCheck]::Click($PillX, $PillY); Start-Sleep -Milliseconds 700; State "click pill to close"
[CardCheck]::SetCursorPos($orig.X, $orig.Y) | Out-Null
