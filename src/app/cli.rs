//! Lightweight CLI used by the release smoke check and by users in a
//! terminal. Returns true when the process was fully handled and the GUI
//! must not start.

/// Attach to the parent console so `--version` / `--check-update` output is
/// visible when launched from a terminal. The GUI build sets
/// `windows_subsystem = "windows"`, which otherwise detaches stdio.
#[cfg(windows)]
fn attach_parent_console() {
    use windows::Win32::System::Console::AttachConsole;
    unsafe {
        let _ = AttachConsole(0xFFFFFFFF);
    }
}

pub(crate) fn handle_cli() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        #[cfg(windows)]
        attach_parent_console();
        println!("keyboard-chattering-fix {}", crate::config::VERSION);
        return true;
    }
    if args.iter().any(|a| a == "--check-language") {
        #[cfg(windows)]
        attach_parent_console();
        // What "Auto" resolves to on this PC (support aid and E2E hook).
        let name = crate::i18n::system_locale_name();
        let lang = crate::i18n::system_language();
        println!(
            "system language: {} -> {}",
            name.as_deref().unwrap_or("unknown"),
            lang.code()
        );
        return true;
    }
    if args.iter().any(|a| a == "--check-helper") {
        #[cfg(windows)]
        attach_parent_console();
        let described = std::env::current_exe()
            .map_err(|e| {
                crate::updater::UpdateError::with_detail(
                    crate::updater::UpdateErrorKind::ExePath,
                    e,
                )
            })
            .and_then(|exe| crate::updater::describe_helper(&exe));
        match described {
            Ok(text) => println!("helper ok: {text}"),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return true;
    }
    if args.iter().any(|a| a == "--check-update") {
        #[cfg(windows)]
        attach_parent_console();
        match crate::updater::check_for_update() {
            Ok(manifest) => println!("update available {}", manifest.version),
            Err(crate::updater::UpdateError::NoUpdate) => println!("up to date"),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return true;
    }
    // Control the running instance (P2-4): `--toggle`, `--pause-15`,
    // `--pause-60`, `--resume`, `--cleaning` are forwarded to it with
    // `WM_COPYDATA` instead of starting a second copy. Only protection and
    // cleaning commands are accepted; exit/update are never forwarded.
    #[cfg(windows)]
    if let Some(flag) = args
        .iter()
        .find(|a| super::instance::control_command(a).is_some())
    {
        attach_parent_console();
        let cmd = super::instance::control_command(flag).expect("matched above");
        match super::instance::forward_control(cmd) {
            super::instance::ForwardOutcome::Sent => {
                println!("sent {flag} to the running instance");
            }
            super::instance::ForwardOutcome::NotRunning => {
                eprintln!("no running instance of KeyboardChatteringFix was found");
                std::process::exit(1);
            }
            super::instance::ForwardOutcome::Failed => {
                eprintln!(
                    "the running instance did not take the command \
                     (administrator window? run this command with the same elevation)"
                );
                std::process::exit(1);
            }
        }
        return true;
    }
    false
}
