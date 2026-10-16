# GEMINI.md — KeyboardChatteringFix (`kcfix-rs`)

Real-time keyboard-chatter filter for Windows: a low-level hook (`WH_KEYBOARD_LL`)
drops bounced presses, with a native Win32 UI and an Ed25519-signed auto-update
pipeline. Single binary + `kcfix-updater` helper (Cargo workspace). Reply in Korean.

## Where things live

| Area | Modules |
|---|---|
| Startup | `src/app/`: `bootstrap`, `cli`, `instance`, `state` |
| Hot input path | `src/input/`: `hook`, `sound` |
| Settings | `src/config/`: `model`, `storage`, `autostart`, `sound` |
| Filter core | `src/debounce.rs` |
| Strings (KO+EN pairs) | `src/i18n/` |
| Window/tray/paint | `src/ui/`: `window`, `stats`, `text`, `settings_form`, `tray`, `update`, `paint`, … |
| Update client/helper | `src/updater/`, `crates/kcfix-updater/` |

## Performance contracts (do not regress)

1. **Hook**: lock + `decide` only — no allocation, no file I/O, no Win32 calls.
   Sound uses the pre-encoded `Shared.sound_file.wide` buffer.
2. **500 ms timer**: snapshot under a short lock → `render_stats` (pure) →
   `diff_stats` → update only changed controls. Never call Win32 while
   holding the shared lock.
3. **Paint**: use cached `theme_is_dark()`; registry reads happen only in
   `refresh_theme()` (startup + `WM_SETTINGCHANGE`).
4. Keep render/diff logic HWND-free and covered by `stats.rs` unit tests.

## Verify before finishing

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
```

(`.githooks/pre-push` runs the same three gates.)

## Conventions that tests enforce

- `config::VERSION` == `Cargo.toml` version == `v{VERSION}` tag (`version_sync.rs`).
- No `KCFIX_DBG_*`/ad-hoc log code in commits; no GUI-opening tests
  (they freeze the runner — only `--version`/`-V` smoke tests).
- Re-export only what is used; every visible string needs KO+EN.
- Prefer tightening an existing assertion over adding a hollow test.

## Open risks

- Past report: GUI "not responding" right after launch (timer-tick
  `SetWindowTextW` path suspected). Recent changes shrink that surface
  (no Win32 under lock, diffed updates) but the root cause is unconfirmed.
- `second_acquisition_fails_while_first_held` fails next to a live tray app.
- Per-key thresholds (`key.XX`) are config-file only; the event log buffer
  is recorded but not displayed.
