param([string]$Out, [string]$Title, [long]$Handle = 0)
# Screenshots the window whose title matches $Title (or the foreground window).
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
}
"@
[W]::SetProcessDPIAware() | Out-Null
$h = if ($Handle) { [IntPtr]$Handle } elseif ($Title) { [W]::FindWindow($null, $Title) } else { [W]::GetForegroundWindow() }
$r = New-Object W+RECT
[W]::GetWindowRect($h, [ref]$r) | Out-Null
$pad = 24
$x = $r.L - $pad; $y = $r.T - $pad; $w = ($r.R - $r.L) + 2*$pad; $hh = ($r.B - $r.T) + 2*$pad
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size $w, $hh))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
"rect: $($r.L),$($r.T) -> $($r.R),$($r.B)"

