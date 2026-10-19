use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    cli::{self, CompilationMode},
    link,
    manifest::{Manifest, PackageType},
    target::Target,
};

use super::{
    SOURCE_DIRECTORY, TYPE_BIN_ENTRY, TYPE_LIB_ENTRY, apply_link_args, apply_pkg_args, cmd_check,
    compiler_command, ensure_dependencies, fingerprint, valid_output_name,
};

type RunArtifact = PathBuf;

pub fn cmd_build(args: cli::BuildArgs, manifest: &Manifest, root: &Path) -> Option<RunArtifact> {
    let lockfile = cmd_check(manifest, root, true);
    let mode = args.mode.resolve();

    // basic dist directory
    fs::create_dir_all(root.join("dist")).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to create `dist` directory: {err}"));
    });
    let dist_dir = root.join("dist");

    // output target directory (for host its still dist)
    let (target_dir, needs_create) = match &args.target {
        Some(target) => (dist_dir.join(target), true),
        None => (dist_dir.to_path_buf(), false),
    };

    if needs_create {
        fs::create_dir_all(&target_dir).unwrap_or_else(|err| {
            cli::exit_with_error(format!(
                "Unable to create `{}` directory: {err}",
                target_dir.to_string_lossy()
            ));
        })
    }

    // output mode directory
    let output_dir = match mode {
        CompilationMode::Debug => target_dir.join("debug"),
        CompilationMode::Release => target_dir.join("release"),
    };

    fs::create_dir_all(&output_dir).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Unable to create `{}` directory: {err}",
            output_dir.to_string_lossy()
        ));
    });

    // output file name and flags
    let target = Target::resolve(args.target.as_deref());

    let (output_name, compiler_flags) = match manifest.build.as_ref() {
        Some(build) => {
            let selected = build.select(&target);

            (
                selected
                    .output
                    .unwrap_or_else(|| default_output_name(manifest)),
                selected.flags,
            )
        }
        None => (default_output_name(manifest), Vec::new()),
    };
    let windows_target = target.is_windows();

    let output_path = output_dir.join(match manifest.package.r#type {
        PackageType::Bin if windows_target => format!("{output_name}.exe"),
        _ => output_name.clone(),
    });

    let artifact_path = build_artifact_path(
        &manifest.package.r#type,
        &args,
        &output_dir,
        &output_path,
        &output_name,
        windows_target,
    );

    let fingerprint_path = output_dir.join(fingerprint::FINGERPRINT_FILE);
    let compiler_version = super::ZEEN_VERSION.0.to_string();
    let mode_name = mode.to_string();
    let emit = args.emit.map(|emit| emit.to_string());
    let current_fingerprint = fingerprint::compute(
        root,
        &fingerprint::BuildInputs {
            compiler_version: &compiler_version,
            mode: &mode_name,
            target: args.target.as_deref(),
            emit: emit.as_deref(),
        },
    );

    if fingerprint::is_current(&fingerprint_path, &current_fingerprint, &artifact_path) {
        ensure_dependencies(&lockfile);

        cli::println_info(
            "Finished",
            format!("`{}` (cached)", artifact_path.to_string_lossy()),
        );

        return match manifest.package.r#type {
            PackageType::Bin => Some(output_path),
            PackageType::Static | PackageType::Dynamic => None,
        };
    }

    let src_dir = root.join(SOURCE_DIRECTORY);
    let mut compiler_cmd = compiler_command(&src_dir);

    // final compilation

    match manifest.package.r#type {
        PackageType::Bin => {
            compiler_cmd
                .arg(TYPE_BIN_ENTRY)
                .arg(&output_path)
                .args(compiler_flags);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            compiler_cmd.arg("--mode").arg(mode.to_string());
            apply_pkg_args(&mut compiler_cmd, &lockfile, root);
            apply_link_args(&mut compiler_cmd, &lockfile, root, &target);

            let success = compiler_cmd.status().map(|x| x.success()).unwrap_or(false);

            if !success {
                std::process::exit(1);
            }

            fingerprint::store(&fingerprint_path, &current_fingerprint);

            Some(output_path)
        }
        PackageType::Static => {
            let triple = args.target.as_deref();

            if let Some(emit) = args.emit {
                let mut emit_command = compiler_command(&src_dir);

                emit_command
                    .arg(TYPE_LIB_ENTRY)
                    .arg(&output_path)
                    .arg("--mode")
                    .arg(mode.to_string())
                    .arg("--emit")
                    .arg(emit.to_string());

                if let Some(triple) = triple {
                    emit_command.arg("--target").arg(triple);
                }

                apply_pkg_args(&mut emit_command, &lockfile, root);
                apply_link_args(&mut emit_command, &lockfile, root, &target);

                let success = emit_command
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if !success {
                    std::process::exit(1);
                }

                fingerprint::store(&fingerprint_path, &current_fingerprint);

                cli::println_info("Emitted", format!("{}", output_path.to_string_lossy()));

                return None;
            }

            compiler_cmd
                .arg(TYPE_LIB_ENTRY)
                .arg(&output_path)
                .args(compiler_flags)
                .arg("--mode")
                .arg(mode.to_string())
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            apply_pkg_args(&mut compiler_cmd, &lockfile, root);
            apply_link_args(&mut compiler_cmd, &lockfile, root, &target);

            let success = compiler_cmd.status().map(|x| x.success()).unwrap_or(false);

            if !success {
                std::process::exit(1);
            }

            let obj_file = output_dir.join(format!(
                "{output_name}.{}",
                if triple.unwrap_or_default().contains("msvc")
                    || triple.unwrap_or_default().contains("windows")
                    || (triple.is_none() && cfg!(target_os = "windows"))
                {
                    "obj"
                } else {
                    "o"
                }
            ));

            let archived = link::archive_static(
                std::slice::from_ref(&obj_file),
                &output_dir.join(link::static_library_name(&output_name, triple)),
                triple,
            );

            fingerprint::store(&fingerprint_path, &current_fingerprint);

            cli::println_info(
                "Archived",
                format!("static library: `{}`", archived.to_string_lossy()),
            );

            None
        }
        PackageType::Dynamic => {
            let triple = args.target.as_deref();

            if let Some(emit) = args.emit {
                let mut emit_command = compiler_command(&src_dir);

                emit_command
                    .arg(TYPE_LIB_ENTRY)
                    .arg(&output_path)
                    .arg("--mode")
                    .arg(mode.to_string())
                    .arg("--emit")
                    .arg(emit.to_string());

                if let Some(triple) = triple {
                    emit_command.arg("--target").arg(triple);
                }

                apply_pkg_args(&mut emit_command, &lockfile, root);
                apply_link_args(&mut emit_command, &lockfile, root, &target);

                let success = emit_command
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if !success {
                    std::process::exit(1);
                }

                fingerprint::store(&fingerprint_path, &current_fingerprint);

                cli::println_info("Emitted", format!("{}", output_path.to_string_lossy()));

                return None;
            }

            compiler_cmd
                .arg(TYPE_LIB_ENTRY)
                .arg(&output_path)
                .args(compiler_flags)
                .arg("--mode")
                .arg(mode.to_string())
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            apply_pkg_args(&mut compiler_cmd, &lockfile, root);
            apply_link_args(&mut compiler_cmd, &lockfile, root, &target);

            let success = compiler_cmd.status().map(|x| x.success()).unwrap_or(false);

            if !success {
                std::process::exit(1);
            }

            let obj_file = output_dir.join(format!(
                "{output_name}.{}",
                if triple.unwrap_or_default().contains("msvc")
                    || triple.unwrap_or_default().contains("windows")
                    || (triple.is_none() && cfg!(target_os = "windows"))
                {
                    "obj"
                } else {
                    "o"
                }
            ));

            let linked = link::link_dynamic(
                std::slice::from_ref(&obj_file),
                &output_dir.join(link::dynamic_library_name(&output_name, triple)),
                triple,
            );

            fingerprint::store(&fingerprint_path, &current_fingerprint);

            cli::println_info(
                "Linked",
                format!("dynamic library: `{}`", linked.to_string_lossy()),
            );

            None
        }
    }
}

fn default_output_name(manifest: &Manifest) -> String {
    let name = manifest.package.name.trim().to_string();

    if !valid_output_name(&name) {
        cli::exit_with_error(format!(
            "Package name `{name}` is not a valid output name, rename the package or set `[build].output`"
        ));
    }

    name
}

fn build_artifact_path(
    package_type: &PackageType,
    args: &cli::BuildArgs,
    output_dir: &Path,
    output_path: &Path,
    output_name: &str,
    windows_target: bool,
) -> PathBuf {
    match package_type {
        PackageType::Bin => output_path.to_path_buf(),
        PackageType::Static => match args.emit {
            Some(emit) => emitted_artifact_path(output_path, emit, windows_target),
            None => output_dir.join(link::static_library_name(
                output_name,
                args.target.as_deref(),
            )),
        },
        PackageType::Dynamic => match args.emit {
            Some(emit) => emitted_artifact_path(output_path, emit, windows_target),
            None => output_dir.join(link::dynamic_library_name(
                output_name,
                args.target.as_deref(),
            )),
        },
    }
}

fn emitted_artifact_path(
    output_path: &Path,
    emit: cli::CompilationOutput,
    windows_target: bool,
) -> PathBuf {
    match emit {
        cli::CompilationOutput::Binary => output_path.to_path_buf(),
        cli::CompilationOutput::Object => {
            output_path.with_extension(if windows_target { "obj" } else { "o" })
        }
        cli::CompilationOutput::EmitIR => output_path.with_extension("ll"),
        cli::CompilationOutput::EmitMIR => output_path.with_extension("mir"),
    }
}

pub fn cmd_run(args: cli::RunArgs, manifest: &Manifest, root: &Path) {
    if manifest.package.r#type != PackageType::Bin {
        cli::exit_with_error("Run command isn't supported for library packages");
    }

    let Some(run_artifact) = cmd_build(
        cli::BuildArgs {
            mode: args.mode,
            target: None,
            emit: None,
        },
        manifest,
        root,
    ) else {
        cli::exit_with_error("No build artifact provided to run");
    };

    let cmd = Command::new(run_artifact)
        .args(&args.program_args)
        .status()
        .unwrap_or_else(|err| cli::exit_with_error(format!("Build execution failed: {}", err)));

    std::process::exit(cmd.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn build_args(target: Option<&str>, emit: Option<cli::CompilationOutput>) -> cli::BuildArgs {
        cli::BuildArgs {
            mode: cli::ModeArgs {
                mode: cli::CompilationMode::Debug,
                release: false,
                debug: false,
            },
            target: target.map(str::to_owned),
            emit,
        }
    }

    #[test]
    fn binary_artifact_is_the_output_file() {
        let directory = PathBuf::from("/project/dist/debug");
        let output = directory.join("app");

        let artifact = build_artifact_path(
            &PackageType::Bin,
            &build_args(None, Some(cli::CompilationOutput::EmitIR)),
            &directory,
            &output,
            "app",
            false,
        );

        assert_eq!(artifact, output);
    }

    #[test]
    fn static_artifact_is_the_archive() {
        let directory = PathBuf::from("/project/dist/debug");
        let output = directory.join("app");

        let artifact = build_artifact_path(
            &PackageType::Static,
            &build_args(None, None),
            &directory,
            &output,
            "app",
            false,
        );

        assert_eq!(artifact, directory.join("libapp.a"));
    }

    #[test]
    fn emitted_artifacts_follow_compiler_extensions() {
        let output = PathBuf::from("/project/dist/debug/app");

        assert_eq!(
            emitted_artifact_path(&output, cli::CompilationOutput::EmitIR, false),
            PathBuf::from("/project/dist/debug/app.ll")
        );

        assert_eq!(
            emitted_artifact_path(&output, cli::CompilationOutput::Object, false),
            PathBuf::from("/project/dist/debug/app.o")
        );

        assert_eq!(
            emitted_artifact_path(&output, cli::CompilationOutput::Object, true),
            PathBuf::from("/project/dist/debug/app.obj")
        );

        assert_eq!(
            emitted_artifact_path(&output, cli::CompilationOutput::EmitMIR, false),
            PathBuf::from("/project/dist/debug/app.mir")
        );
    }
}
