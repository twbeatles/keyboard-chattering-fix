# End-to-end checks (real binaries)

Windows-only PowerShell checks for behavior that unit tests cannot reach:
the real low-level hook, session end, and the update swap. They are **not**
part of `cargo test` (they start the GUI app), so run them by hand before a
release, from the repo root:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\hook_session_e2e.ps1 -Build
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\update_swap_e2e.ps1
# Current app against a helper users already have installed:
gh release download v0.2.0 -p kcfix-updater.exe -D $env:TEMP\kcfix_old_helper
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\update_swap_e2e.ps1 -HelperExe $env:TEMP\kcfix_old_helper\kcfix-updater.exe
```

| Script | Verifies |
|---|---|
| `hook_session_e2e.ps1` | Synthesized F24 chatter is blocked by the real hook; filtering keeps working while the UI thread is **suspended** (hook on its own thread); `WM_ENDSESSION` persists counters before a no-`WM_DESTROY` kill; an external `key.41` edit survives that save. `-AppExe` runs it against another build (an older commit fails the stall and persistence checks). |
| `update_swap_e2e.ps1` | Two consecutive swaps with the real helper: hash re-check, rename, relaunch + readiness handshake, no `.exe.old`/staging leftovers, and `--check-helper` still accepting the helper that stays in place. `-HelperExe` tests a published helper. |

CI에서는 Actions → **e2e** → Run workflow(수동 전용, `.github/workflows/e2e.yml`)가
같은 스크립트를 돌리며, 직전에 게시된 헬퍼로도 교체를 검증합니다.

Safety: every run uses a throw-away `APPDATA`, the only injected key is F24,
and a running real instance aborts the run. Do not type during
`hook_session_e2e` (~6 s). Exit code 0 means every check passed.
