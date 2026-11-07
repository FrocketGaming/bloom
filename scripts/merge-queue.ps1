param([int[]]$Prs)
# Merges pull requests one at a time: bring up to date with main, wait for CI, squash-merge.
foreach ($n in $Prs) {
  "=== PR #$n"
  $info = gh pr view $n --json state,mergeStateStatus | ConvertFrom-Json
  if ($info.state -ne 'OPEN') { "skipped: $($info.state)"; continue }
  gh pr update-branch $n 2>&1 | Out-String
  Start-Sleep 20
  gh pr checks $n --watch --fail-fast --interval 30 2>&1 | Select-Object -Last 3 | Out-String
  if ($LASTEXITCODE -ne 0) { "CI FAILED on #$n - stopping"; exit 1 }
  gh pr merge $n --squash 2>&1 | Out-String
  $state = (gh pr view $n --json state | ConvertFrom-Json).state
  "result: $state"
  if ($state -ne 'MERGED') { "MERGE FAILED on #$n - stopping"; exit 1 }
}
"ALL DONE"
