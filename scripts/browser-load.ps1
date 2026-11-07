# Total CPU seconds used over a 15-second window, and memory, for Chrome and Edge.
$names = 'chrome', 'msedge'
function Snapshot { Get-Process -Name $names -ErrorAction SilentlyContinue }
$before = Snapshot
if (-not $before) { "No Chrome or Edge running"; return }
$cpu0 = ($before | Measure-Object CPU -Sum).Sum
Start-Sleep -Seconds 15
$after = Snapshot
$cpu1 = ($after | Measure-Object CPU -Sum).Sum
foreach ($n in $names) {
  $p = $after | Where-Object ProcessName -eq $n
  if ($p) { "{0}: {1} processes, {2:N0} MB memory" -f $n, $p.Count, (($p | Measure-Object WorkingSet64 -Sum).Sum / 1MB) }
}
"CPU used in 15 s: {0:N2} s (about {1:N1}% of one core)" -f ($cpu1 - $cpu0), (($cpu1 - $cpu0) / 15 * 100)
