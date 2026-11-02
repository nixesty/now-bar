# Opens a borderless window covering the primary monitor (like a borderless game or F11 video)
# and reports whether the pill is hidden meanwhile and back afterwards. Starts pebbly --demo.
param([string]$Exe = "$PSScriptRoot/../target/quick/pebbly.exe")
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Fc {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string n);
  [DllImport("user32.dll")] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string c, string n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  // Windows only lets a window take the foreground after real input, so click it like a user.
  public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(2,0,0,0,IntPtr.Zero); System.Threading.Thread.Sleep(80); mouse_event(4,0,0,0,IntPtr.Zero); }
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out R r, int s);
  public static string Fg() { var h = GetForegroundWindow(); var sb = new System.Text.StringBuilder(256); GetClassName(h, sb, 256); R r; DwmGetWindowAttribute(h, 9, out r, 16); return String.Format("{0} ({1},{2})-({3},{4})", sb, r.L, r.T, r.Rt, r.B); }
  public static string Pill() {
    IntPtr bar = FindWindow("Shell_TrayWnd", null);
    IntPtr input = FindWindowEx(bar, IntPtr.Zero, "PebblyPillInput", null);
    return input == IntPtr.Zero ? "missing" : (IsWindowVisible(input) ? "visible" : "hidden");
  }
}
"@
[Fc]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
Get-Process pebbly -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process $Exe -ArgumentList "--demo" | Out-Null; Start-Sleep -Milliseconds 1500
"before:              pill " + [Fc]::Pill()
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = 'None'; $form.StartPosition = 'Manual'; $form.BackColor = 'Black'; $form.TopMost = $true
$form.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$form.Show(); $form.Activate(); [System.Windows.Forms.Application]::DoEvents()
Start-Sleep -Milliseconds 200; [Fc]::Click(960, 500)
for ($i = 0; $i -lt 6; $i++) { Start-Sleep -Milliseconds 100; [System.Windows.Forms.Application]::DoEvents() }
Start-Sleep -Milliseconds 600; [System.Windows.Forms.Application]::DoEvents()
"fullscreen window:   pill " + [Fc]::Pill() + "   foreground: " + [Fc]::Fg()
$form.Close(); $form.Dispose(); [System.Windows.Forms.Application]::DoEvents()
Start-Sleep -Milliseconds 800
"after closing it:    pill " + [Fc]::Pill()
