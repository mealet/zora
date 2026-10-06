use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    cli,
    manifest::{self, Manifest},
};

const COMPILER_COMMAND: &str = "zeen";

/// Verify that Zeen compiler is installed
pub fn verify_zeen_installed() {
    let available = Command::new(COMPILER_COMMAND)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());

    if !available {
        cli::exit_with_error(
            "Zeen compiler is not found, please verify installation (check `zeen --version`)",
        );
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

        cli::exit_with_error(error_display);
    })
}

/// Compiler's command with environment
fn compiler_command(src_dir: &Path) -> Command {
    let mut command = Command::new(COMPILER_COMMAND);
    let _ = command.current_dir(src_dir);

    command
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
    const SOURCE_DIRECTORY: &str = "src";

    const TYPE_BIN_ENTRY: &str = "main.zn";
    const TYPE_LIB_ENTRY: &str = "index.zn";

    const AVAILABLE_BUILD_FLAGS: &[&str] = &[];

    // 1. Paths checks

    let src_available = root.join("src").is_dir();

    if !src_available {
        cli::exit_with_error("Source directory `src` not found");
    }

    let entry_file = match manifest.package.r#type {
        manifest::PackageType::Bin => TYPE_BIN_ENTRY,
        manifest::PackageType::Static | manifest::PackageType::Dynamic => TYPE_LIB_ENTRY,
    };
    let entry_path = root.join(SOURCE_DIRECTORY).join(entry_file);

    if !entry_path.is_file() {
        cli::exit_with_error(format!(
            "Entry source not found (for your package its `{}/{}`)",
            SOURCE_DIRECTORY, entry_file
        ));
    }

    // 2. Build section checks

    if let Some(ref build_section) = manifest.build {
        let output = &build_section.output;
        let output_verification = !(output.contains(".")
            || output.contains(" ")
            || output.contains("/")
            || output.contains("\\")
            || output.is_empty());

        if !output_verification {
            cli::exit_with_error("Field `[build].output` should have only name without extension");
        }

        if let Some(ref flags) = build_section.flags {
            flags.iter().for_each(|flag| {
                if !AVAILABLE_BUILD_FLAGS.contains(&flag.as_str()) {
                    cli::exit_with_error(format!("Field `[build].flags` doesn't have '{}'", flag));
                }
            });
        }
    }

    // TODO: 3. Dependencies check

    // 4. Zeen sources check (via `--check` flag)

    let src_dir = root.join(SOURCE_DIRECTORY);

    let check_command = compiler_command(&src_dir)
        .arg(entry_file)
        .arg("--check")
        .status()
        .map(|status| status.success())
        .unwrap_or_else(|err| cli::exit_with_error(format!("Check command error: {}", err)));

    if !check_command {
        std::process::exit(1);
    }
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
