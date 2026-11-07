# Developer scripts

Windows PowerShell helpers for testing Bloom by hand. Start Bloom first; the
typing tests open a small window and type into it for a few seconds, so leave
the keyboard and mouse alone while they run.

| Script | What it does |
| --- | --- |
| `smoke.ps1` | Types a few triggers into a text box and shows what they became. |
| `password-test.ps1` | Types `;date ` into a normal box and a password box. It should expand only in the normal box. |
| `browser-load.ps1` | Measures Chrome and Edge CPU and memory over 15 seconds. |
| `crop.ps1 -Out shot.png [-Title name]` | Screenshots one window. |
| `merge-queue.ps1 -Prs 12,13` | For each pull request: update from main, wait for CI, squash-merge. |