# Renders a visible top-level window of a process to PNG with PrintWindow (works across monitors/DPI).
param([string]$Process = 'sticky', [string]$Title, [string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class Grab {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public static IntPtr Find(uint pid, string title) { IntPtr found = IntPtr.Zero; EnumWindows((h,l)=>{ uint p; GetWindowThreadProcessId(h,out p); if(p==pid && IsWindowVisible(h)){ var sb=new StringBuilder(200); GetWindowText(h,sb,200); if (sb.ToString().Contains(title)) { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
}
'@
[Grab]::SetThreadDpiAwarenessContext([IntPtr]-4) | Out-Null  # per-monitor v2
$h = [IntPtr]::Zero
foreach ($p in Get-Process $Process) { $h = [Grab]::Find([uint32]$p.Id, $Title); if ($h -ne [IntPtr]::Zero) { break } }
if ($h -eq [IntPtr]::Zero) { throw "window '$Title' not found" }
$r = New-Object Grab+RECT; [Grab]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
$g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
[Grab]::PrintWindow($h, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc); $bmp.Save($Out)
"$($bmp.Width)x$($bmp.Height) at ($($r.L),$($r.T))-($($r.R),$($r.B)) -> $Out"

