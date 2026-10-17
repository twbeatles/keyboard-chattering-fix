#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use kcfix_updater::{
    discard_replacement, relaunch_previous, should_relaunch_previous, update_executable_with,
    HelperLang, UpdateOptions,
};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "kcfix-updater", about = "Keyboard Chatter Fix updater helper")]
struct Cli {
    #[arg(long, default_value_t = 0)]
    pid: u32,

    #[arg(long)]
    current: PathBuf,

    #[arg(long)]
    replacement: PathBuf,

    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,

    #[arg(long, default_value_t = false)]
    no_relaunch: bool,

    /// Argument to pass to the relaunched app. Repeat for multiple arguments.
    #[arg(long = "relaunch-arg")]
    relaunch_arg: Vec<String>,

    /// Expected SHA-256 of the replacement, re-verified immediately before the
    /// swap. The app already verified the download; this covers the gap while
    /// the staged file sits in %TEMP% waiting for the helper to start.
    #[arg(long)]
    sha256: Option<String>,
}

fn show_error_dialog(message: &str, lang: HelperLang) {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let text = HSTRING::from(message);
        let title = HSTRING::from(lang.failure_title());
        unsafe {
            let _ = MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = lang;
        eprintln!("Update Error: {message}");
    }
}

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .try_init();

    let args = Cli::parse();
    let relaunch = !args.no_relaunch;

    match update_executable_with(
        &args.current,
        &args.replacement,
        args.pid,
        Duration::from_secs(args.timeout_secs),
        &UpdateOptions {
            relaunch,
            relaunch_args: args.relaunch_arg.clone(),
            expected_sha256: args.sha256.clone(),
            readiness_timeout: args.sha256.as_ref().map(|_| Duration::from_secs(60)),
        },
    ) {
        Ok(()) => {
            info!("update helper finished successfully");
            std::process::exit(0);
        }
        Err(err) => {
            error!(%err, "update helper failed");
            discard_replacement(&args.replacement);
            // Start the previous version first so the user is not left
            // without protection while the error dialog is open.
            let relaunched = relaunch
                && should_relaunch_previous(&err)
                && relaunch_previous(&args.current, &args.relaunch_arg);
            let lang = HelperLang::from_env();
            let message = if relaunched {
                format!("{}\n\n{}", err.message(lang), lang.relaunched_note())
            } else {
                err.message(lang)
            };
            show_error_dialog(&message, lang);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's `helper_args` (`src/updater/staging.rs`) sends exactly this
    /// shape. The helper beside an installed app is never replaced by an
    /// in-app update, so this command line is an append-only contract: every
    /// released helper must keep accepting it.
    const APP_ARGS: &[&str] = &[
        "kcfix-updater",
        "--pid",
        "42",
        "--current",
        r"C:\Apps\KeyboardChatteringFix.exe",
        "--replacement",
        r"C:\Temp\KeyboardChatteringFix_update_0.3.0_1_2_3.exe",
        "--timeout-secs",
        "120",
        "--sha256",
        "abababababababababababababababababababababababababababababababab",
        "--relaunch-arg=--minimized",
    ];

    #[test]
    fn accepts_every_flag_the_app_sends() {
        let cli = Cli::try_parse_from(APP_ARGS).expect("app command line must parse");
        assert_eq!(cli.pid, 42);
        assert_eq!(cli.timeout_secs, 120);
        assert!(cli.sha256.is_some());
        assert_eq!(cli.relaunch_arg, vec!["--minimized".to_string()]);
        assert!(!cli.no_relaunch);
    }

    #[test]
    fn hyphen_values_need_the_equals_form() {
        // `--relaunch-arg --minimized` reads `--minimized` as an unknown
        // flag, so the app must send `--relaunch-arg=--minimized` (which
        // every clap-4 helper, old or new, accepts).
        let mut split: Vec<&str> = APP_ARGS[..APP_ARGS.len() - 1].to_vec();
        split.extend(["--relaunch-arg", "--minimized"]);
        assert!(Cli::try_parse_from(split).is_err());
    }
}
