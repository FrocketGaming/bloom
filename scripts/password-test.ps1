# Types ;date into a normal box and a password box, and reports what Bloom did.
Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = "Bloom password test"; $form.Width = 460; $form.Height = 200; $form.TopMost = $true
$normal = New-Object System.Windows.Forms.TextBox
$normal.Left = 20; $normal.Top = 20; $normal.Width = 400
$secret = New-Object System.Windows.Forms.TextBox
$secret.Left = 20; $secret.Top = 70; $secret.Width = 400; $secret.UseSystemPasswordChar = $true
$form.Controls.AddRange(@($normal, $secret))
$form.Show(); $form.Activate()
function Pump($ms) { $end = (Get-Date).AddMilliseconds($ms); while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 } }
function TypeSlow($text) { foreach ($c in $text.ToCharArray()) { [System.Windows.Forms.SendKeys]::SendWait("$c"); Pump 50 } }
Pump 700
$normal.Focus() | Out-Null; Pump 400
TypeSlow ';date '; Pump 1200
$secret.Focus() | Out-Null; Pump 600
TypeSlow ';date '; Pump 1200
$normal.Focus() | Out-Null; Pump 600
TypeSlow ' ;date '; Pump 1200
$form.Close()
"Normal box (typed ';date ' then ' ;date ' after leaving the password box): '$($normal.Text)'"
"Password box (typed ';date '): '$($secret.Text)'"
