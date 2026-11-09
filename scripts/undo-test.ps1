# Checks that undoing an expansion with Backspace survives fast typing.
# Start the Bloom build you want to test first. This adds a test snippet
# (;uv -> "uv run main.py") to your snippets folder for the run and removes it
# afterwards. It expands the snippet in a small window, presses Backspace to
# undo it, then types x after a short delay. With undo on in Settings, the
# result should be ";uvx" every time. Leave the keyboard and mouse alone while
# it runs (about 30 seconds).
param([int[]]$Delays = @(-1, 15, 30, 45, 250), [int]$Runs = 2)
$snippet = Join-Path $env:APPDATA "Bloom\snippets\zz-undo-test.toml"
@'
[[snippet]]
trigger = ";uv"
name = "undo test"
body = "uv run main.py"
'@ | Set-Content -Encoding utf8 $snippet

Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = "Bloom undo test"; $form.Width = 520; $form.Height = 200; $form.TopMost = $true
$box = New-Object System.Windows.Forms.TextBox
$box.Multiline = $true; $box.Dock = 'Fill'; $box.Font = New-Object System.Drawing.Font("Consolas", 12)
$form.Controls.Add($box)
$form.Show(); $form.Activate(); $box.Focus() | Out-Null
function Pump($ms) { $end = (Get-Date).AddMilliseconds($ms); while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 5 } }
function TypeSlow($text) { foreach ($c in $text.ToCharArray()) { [System.Windows.Forms.SendKeys]::SendWait("$c"); Pump 40 } }

$rows = @()
try {
  Pump 1500   # give Bloom time to pick up the new snippet
  foreach ($d in $Delays) {
    for ($r = 0; $r -lt $Runs; $r++) {
      $box.Clear(); Pump 200
      TypeSlow ';uv'
      Pump 900
      [System.Windows.Forms.SendKeys]::SendWait("{BACKSPACE}")
      $expected = ';uv'
      if ($d -ge 0) { Pump $d; [System.Windows.Forms.SendKeys]::SendWait("x"); $expected = ';uvx' }
      Pump 1500
      $rows += [pscustomobject]@{
        DelayMs = $(if ($d -lt 0) { 'no x' } else { $d }); Expected = $expected; Got = $box.Text
        Pass = ($box.Text -eq $expected)
      }
    }
  }
} finally {
  $form.Close()
  Remove-Item $snippet -ErrorAction SilentlyContinue
}
$rows | Format-Table -AutoSize | Out-String -Width 200
"Passed {0} of {1}" -f @($rows | Where-Object Pass).Count, $rows.Count
