# Types triggers into a throwaway text box and reports what Bloom turned them into.
Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = "Bloom smoke test"; $form.Width = 520; $form.Height = 260; $form.TopMost = $true
$box = New-Object System.Windows.Forms.TextBox
$box.Multiline = $true; $box.Dock = 'Fill'; $box.Font = New-Object System.Drawing.Font("Consolas", 12)
$form.Controls.Add($box)
$form.Show(); $form.Activate(); $box.Focus() | Out-Null
function Pump($ms) { $end = (Get-Date).AddMilliseconds($ms); while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 } }
function TypeSlow($text) { foreach ($c in $text.ToCharArray()) { [System.Windows.Forms.SendKeys]::SendWait("$c"); Pump 40 } }
Pump 600
$results = @()
foreach ($case in @(';date ', ';shrug ', 'x;date ', ';sig ')) {
  $box.Clear(); Pump 150
  TypeSlow $case
  Pump 900
  $results += [pscustomobject]@{ Typed = $case; Got = ($box.Text -replace "`r`n", '\n') }
}
$form.Close()
$results | Format-Table -AutoSize | Out-String -Width 200
