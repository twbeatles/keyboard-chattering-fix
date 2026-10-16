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
    false
}
