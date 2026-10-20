//! Recurrence guard for the Rust 2024 migration.
//!
//! Edition 2024 made three things hard errors in this workspace:
//! `gen` became a reserved keyword, `extern` blocks must be `unsafe extern`,
//! and process-env mutation (`set_var`/`remove_var`) requires an `unsafe`
//! block. It also turned `unsafe_op_in_unsafe_fn` into a warn-by-default
//! lint (deny via the repo's `-D warnings` gate) and enabled let-chains,
//! which made `collapsible_if` fire on nested `if let`s. This test locks the
//! manifest side (`edition`/`resolver`/`rust-version`) and the source-side
//! fixes so a revert re-fails here instead of in CI.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("repo root")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[test]
fn workspace_uses_edition_2024_and_resolver_3() {
    let manifest = read("Cargo.toml");
    assert!(
        manifest.contains("edition = \"2024\""),
        "workspace edition must be 2024"
    );
    assert!(
        manifest.contains("resolver = \"3\""),
        "workspace resolver must be 3 for edition 2024"
    );
    assert!(
        manifest.contains("rust-version"),
        "workspace must declare rust-version (>= 1.85 for edition 2024)"
    );
    let helper = read("crates/kcfix-updater/Cargo.toml");
    assert!(
        helper.contains("edition.workspace = true"),
        "kcfix-updater must inherit the workspace edition"
    );
}

#[test]
fn gen_keyword_not_used_as_identifier() {
    let hook = read("src/input/hook.rs");
    assert!(
        !hook.contains("let gen =") && !hook.contains("if gen =="),
        "`gen` is a reserved keyword in edition 2024; use `generation`"
    );
    assert!(
        hook.contains("let generation ="),
        "hook thread generation variable must be named `generation`"
    );
}

#[test]
fn extern_block_is_marked_unsafe() {
    let sound = read("src/input/sound.rs");
    assert!(
        sound.contains("unsafe extern"),
        "PlaySoundW extern block must be `unsafe extern` in edition 2024"
    );
}

#[test]
fn env_mutation_is_wrapped_in_unsafe() {
    for relative in ["src/config/storage.rs", "src/app/bootstrap.rs"] {
        let text = read(relative);
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if (trimmed.contains("std::env::set_var") || trimmed.contains("std::env::remove_var"))
                && !trimmed.starts_with("//")
            {
                let same_line = trimmed.contains("unsafe");
                let prev_is_unsafe_open = index > 0
                    && lines[index - 1].trim() == "unsafe {"
                    && line.starts_with("            ");
                assert!(
                    same_line || prev_is_unsafe_open,
                    "{relative} must wrap env mutation in `unsafe`: {trimmed}"
                );
            }
        }
    }
}
