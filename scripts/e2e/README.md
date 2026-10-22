# End-to-end checks (real binaries)

Windows-only PowerShell checks for behavior that unit tests cannot reach:
the real low-level hook, session end, and the update swap. They are **not**
part of `cargo test` (they start the GUI app), so run them by hand before a
release, from the repo root:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\hook_session_e2e.ps1 -Build
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\features_e2e.ps1
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\update_swap_e2e.ps1
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\language_e2e.ps1
# Current app against a helper users already have installed:
gh release download v0.2.0 -p kcfix-updater.exe -D $env:TEMP\kcfix_old_helper
powershell -ExecutionPolicy Bypass -File .\scripts\e2e\update_swap_e2e.ps1 -HelperExe $env:TEMP\kcfix_old_helper\kcfix-updater.exe
```

| Script | Verifies |
|---|---|
| `hook_session_e2e.ps1` | Synthesized F24 chatter is blocked by the real hook; filtering keeps working while the UI thread is **suspended** (hook on its own thread); `WM_ENDSESSION` persists counters before a no-`WM_DESTROY` kill; an external `key.41` edit survives that save. `-AppExe` runs it against another build (an older commit fails the stall and persistence checks). |
| `features_e2e.ps1` | Release delay: a bounce while F24 is held never shows it released, and the held-back release is sent afterwards; mouse X2 chatter over the app window is blocked; `Ctrl+Alt+F23` toggles protection; a listed app in front (Character Map, skipped when absent) pauses protection with a Home note and resumes after; counters persisted on session end. Moves the mouse over the app window for a moment. |
| `language_e2e.ps1` | `sLanguage` ko/en/auto/legacy/unknown values show the right language in the real window; automatic follows Windows (Korean on a Korean system, English otherwise — non-Korean systems simulated with `KCFIX_LOCALE`); picker changes retranslate at once and are saved; a save without a stored choice writes `sLanguage=auto`. |
| `update_swap_e2e.ps1` | Two consecutive swaps with the real helper: hash re-check, rename, relaunch + readiness handshake, no `.exe.old`/staging leftovers, and `--check-helper` still accepting the helper that stays in place. `-HelperExe` tests a published helper. |

CI에서는 Actions → **e2e** → Run workflow(수동 전용, `.github/workflows/e2e.yml`)가
같은 스크립트를 돌리며, 직전에 게시된 헬퍼로도 교체를 검증합니다.

## Manual checklist: administrator paths

These need a UAC prompt or a sign-in, so no script covers them. Use a test
build under a throw-away `APPDATA`; the `Run` value and the scheduled task
are shared with the real install, so turn both off again afterwards.

1. **One hook only.** Start the app, choose Tools > Run as administrator, then
   start the test build again without elevation. Expected: "already running",
   and Task Manager shows one `KeyboardChatteringFix.exe`.
2. **Sign-in task from a writable folder.** In the elevated copy, choose Tools >
   Start as administrator at sign-in while the exe sits outside `Program Files`.
   Expected: a warning that defaults to No; Yes registers the task (check
   `schtasks /query /tn KeyboardChatteringFix`).
3. **Both entries at sign-in.** Turn on Start with Windows as well, sign out and in.
   Expected: one elevated instance, in the tray; `%TEMP%\kcfix-diag.log` gains nothing.
4. **Broken task fallback.** Disable the task in Task Scheduler, sign out and in.
   Expected: the normal copy starts after about 20 s, and the next sign-in has no delay.
5. **Watchdog and elevated windows.** In the non-elevated copy, keep Task Manager
   in front and type for 30 s. Expected: no "keyboard monitoring stopped" note on
   Home, no restart line in `%TEMP%\kcfix-diag.log`.
6. **No in-app update while elevated.** In the elevated copy, choose Help > Check
   for Updates with a newer release published. Expected: the notice to update from a
   normal start, and no `kcfix-updater_helper_*.exe` in `%TEMP%`.

Safety: every run uses a throw-away `APPDATA`, the only injected key is F24,
and a running real instance aborts the run. Do not type during
`hook_session_e2e` (~6 s). Exit code 0 means every check passed.
