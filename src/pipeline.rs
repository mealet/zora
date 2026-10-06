use std::path::{Path, PathBuf};

use crate::{
    cli,
    manifest::{self, Manifest},
};

/// Verify that Zeen compiler is installed
pub fn verify_zeen_installed() {
    let available = std::process::Command::new("zeen")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());

    if !available {
        cli::println_error(
            "Zeen compiler is not found, please verify installation (check `zeen --version`)",
        );
        std::process::exit(1);
    }
}

/// Layer to load manifest, or display an error if not found
pub fn load_manifest_or_exit() -> (Manifest, PathBuf) {
    Manifest::load().unwrap_or_else(|err| {
        use manifest::LoadError::*;

        let error_display = match err {
            Io(io_error) => io_error.to_string(),
            Toml(toml_error) => toml_error.to_string(),
        };

        cli::println_error(error_display);

        std::process::exit(1);
    })
}

// ==== Initialization Commands ====

pub fn cmd_init(args: &cli::InitArgs) {
    todo!()
}

pub fn cmd_new(args: &cli::NewArgs) {
    todo!()
}

// ==== Manifest Required Commands ====

pub fn cmd_check(manifest: &Manifest, root: &Path) {
    todo!()
}

pub fn cmd_build(args: cli::BuildArgs, manifest: &Manifest, root: &Path) {
    todo!()
}

pub fn cmd_run(args: cli::RunArgs, manifest: &Manifest, root: &Path) {
    todo!()
}

pub fn cmd_add(args: cli::AddArgs, manifest: &Manifest, root: &Path) {
    todo!()
}

pub fn cmd_remove(args: cli::RemoveArgs, manifest: &Manifest, root: &Path) {
    todo!()
}

pub fn cmd_test(manifest: &Manifest, root: &Path) {
    todo!()
}
