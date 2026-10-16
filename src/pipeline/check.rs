use std::path::Path;

use crate::{
    cli,
    lockfile::Lockfile,
    manifest::{self, Manifest},
};

use super::{
    AVAILABLE_BUILD_FLAGS, SOURCE_DIRECTORY, TYPE_BIN_ENTRY, TYPE_LIB_ENTRY, apply_pkg_args,
    compiler_command, ensure_lockfile, valid_output_name, validate_lockfile_or_exit,
};

pub fn cmd_check(manifest: &Manifest, root: &Path, avoid_execution: bool) -> Lockfile {
    // Lockfile and Zeen version checks

    let lockfile = ensure_lockfile(root, manifest);
    validate_lockfile_or_exit(&lockfile);

    // Paths checks

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

    // Build section checks

    if let Some(ref build_section) = manifest.build {
        if !valid_output_name(&build_section.output) {
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

    // Zeen sources check (via `--check` flag)

    if avoid_execution {
        return lockfile;
    }

    let src_dir = root.join(SOURCE_DIRECTORY);

    let mut check_command = compiler_command(&src_dir);
    apply_pkg_args(&mut check_command, &lockfile, root);

    check_command.arg(entry_file).arg("--check");

    if manifest.package.r#type != manifest::PackageType::Bin {
        check_command.arg("--emit").arg("OBJ");
    }

    let check_success = check_command
        .status()
        .map(|status| status.success())
        .unwrap_or_else(|err| cli::exit_with_error(format!("Check command error: {}", err)));

    if !check_success {
        std::process::exit(1);
    }

    lockfile
}
