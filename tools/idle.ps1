# CPU use and memory of the running pebbly.exe over -Seconds (default 10). Doesn't start it.
# Uses QueryProcessCycleTime: TotalProcessorTime only counts whole 15.6 ms scheduler ticks.
param([int]$Seconds = 10)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Cyc { [DllImport("kernel32.dll")] public static extern bool QueryProcessCycleTime(IntPtr h, out ulong cycles); }
"@
$p = Get-Process pebbly -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { "pebbly is not running"; exit 1 }
$mhz = (Get-CimInstance Win32_Processor | Select-Object -First 1).MaxClockSpeed
$c0 = 0; [Cyc]::QueryProcessCycleTime($p.Handle, [ref]$c0) | Out-Null
Start-Sleep -Seconds $Seconds
$c1 = 0; [Cyc]::QueryProcessCycleTime($p.Handle, [ref]$c1) | Out-Null
$p.Refresh()
$ms = ($c1 - $c0) / ($mhz * 1000.0)
"CPU over {0} s: {1:N1} Mcycles (~{2:N1} ms at {3} MHz)   working set: {4:N1} MB   private: {5:N1} MB" -f $Seconds, (($c1 - $c0) / 1e6), $ms, $mhz, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB)
