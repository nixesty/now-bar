# Restarts Explorer (closes open File Explorer windows!) and measures how long until the pill is
# back inside the new taskbar. Run with pebbly.exe --demo already running.
# -Simulate only broadcasts the TaskbarCreated message, without touching Explorer.
param([switch]$Simulate, [int]$TimeoutMs = 20000)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Er {
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string n);
  [DllImport("user32.dll")] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string c, string n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern uint RegisterWindowMessage(string s);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static bool BarUp() {
    IntPtr bar = FindWindow("Shell_TrayWnd", null);
    return bar != IntPtr.Zero && IsWindowVisible(bar);
  }
  public static bool PillIn() {
    IntPtr bar = FindWindow("Shell_TrayWnd", null);
    if (bar == IntPtr.Zero) return false;
    IntPtr input = FindWindowEx(bar, IntPtr.Zero, "PebblyPillInput", null);
    return input != IntPtr.Zero && IsWindowVisible(input);
  }
}
"@
if (-not (Get-Process pebbly -ErrorAction SilentlyContinue)) { "start pebbly.exe --demo first"; exit 1 }
"before: pill in taskbar = " + [Er]::PillIn()
$sw = [Diagnostics.Stopwatch]::StartNew()
if ($Simulate) {
  [Er]::PostMessage([IntPtr]0xFFFF, [Er]::RegisterWindowMessage("TaskbarCreated"), [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
} else {
  Stop-Process -Name explorer -Force; Start-Sleep -Milliseconds 500; Start-Process explorer
}
$seenGone = $Simulate
$barGone = $Simulate
$barBack = -1
while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
  $up = [Er]::BarUp()
  if (-not $up) { $barGone = $true }
  if ($up -and $barGone -and $barBack -lt 0) { $barBack = $sw.ElapsedMilliseconds }
  $in = [Er]::PillIn()
  if (-not $in) { $seenGone = $true }
  if ($in -and $seenGone -and $sw.ElapsedMilliseconds -gt 300) { break }
  Start-Sleep -Milliseconds 25
}
# Windows' own part (the taskbar coming back) and ours (the pill back in it) apart.
"taskbar back after {0} ms" -f $barBack
"after:  pill in taskbar = {0} ({1} ms; {2} ms after the taskbar)" -f [Er]::PillIn(), $sw.ElapsedMilliseconds, ($sw.ElapsedMilliseconds - $barBack)
"pebbly instances: " + @(Get-Process pebbly -ErrorAction SilentlyContinue).Count
