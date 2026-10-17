use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    cli::{self, CompilationMode},
    link,
    manifest::{Manifest, PackageType},
};

use super::{
    SOURCE_DIRECTORY, TYPE_BIN_ENTRY, TYPE_LIB_ENTRY, apply_link_args, apply_pkg_args, cmd_check,
    compiler_command, valid_output_name,
};

type RunArtifact = PathBuf;

pub fn cmd_build(args: cli::BuildArgs, manifest: &Manifest, root: &Path) -> Option<RunArtifact> {
    let lockfile = cmd_check(manifest, root, true);

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
    let output_dir = match args.mode {
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
    let (output_name, compiler_flags) = if let Some(build) = &manifest.build {
        (build.output.clone(), build.flags.clone().unwrap_or(vec![]))
    } else {
        let name = manifest.package.name.trim().to_string();

        if !valid_output_name(&name) {
            cli::exit_with_error(format!(
                "Package name `{name}` is not a valid output name, rename the package or set `[build].output`"
            ));
        }

        (name, vec![])
    };
    let windows_target = args
        .target
        .as_deref()
        .map(|target| target.contains("windows"))
        .unwrap_or(cfg!(target_os = "windows"));

    let output_path = output_dir.join(match manifest.package.r#type {
        PackageType::Bin if windows_target => format!("{output_name}.exe"),
        _ => output_name.clone(),
    });

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

            compiler_cmd.arg("--mode").arg(args.mode.to_string());
            apply_pkg_args(&mut compiler_cmd, &lockfile, root);
            apply_link_args(&mut compiler_cmd, &lockfile, root);

            let success = compiler_cmd.status().map(|x| x.success()).unwrap_or(false);

            if !success {
                std::process::exit(1);
            }

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
                    .arg(args.mode.to_string())
                    .arg("--emit")
                    .arg(emit.to_string());

                if let Some(triple) = triple {
                    emit_command.arg("--target").arg(triple);
                }

                apply_pkg_args(&mut emit_command, &lockfile, root);
                apply_link_args(&mut emit_command, &lockfile, root);

                let success = emit_command
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if !success {
                    std::process::exit(1);
                }

                cli::println_info("Emitted", format!("{}", output_path.to_string_lossy()));

                return None;
            }

            compiler_cmd
                .arg(TYPE_LIB_ENTRY)
                .arg(&output_path)
                .args(compiler_flags)
                .arg("--mode")
                .arg(args.mode.to_string())
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            apply_pkg_args(&mut compiler_cmd, &lockfile, root);
            apply_link_args(&mut compiler_cmd, &lockfile, root);

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
                    .arg(args.mode.to_string())
                    .arg("--emit")
                    .arg(emit.to_string());

                if let Some(triple) = triple {
                    emit_command.arg("--target").arg(triple);
                }

                apply_pkg_args(&mut emit_command, &lockfile, root);
                apply_link_args(&mut emit_command, &lockfile, root);

                let success = emit_command
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if !success {
                    std::process::exit(1);
                }

                cli::println_info("Emitted", format!("{}", output_path.to_string_lossy()));

                return None;
            }

            compiler_cmd
                .arg(TYPE_LIB_ENTRY)
                .arg(&output_path)
                .args(compiler_flags)
                .arg("--mode")
                .arg(args.mode.to_string())
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            apply_pkg_args(&mut compiler_cmd, &lockfile, root);
            apply_link_args(&mut compiler_cmd, &lockfile, root);

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

            cli::println_info(
                "Linked",
                format!("dynamic library: `{}`", linked.to_string_lossy()),
            );

            None
        }
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
