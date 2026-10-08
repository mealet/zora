use crate::cli;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn static_library_name(base_name: &str, target_triple: Option<&str>) -> String {
    if target_triple.is_some_and(|triple| triple.contains("msvc")) {
        format!("{base_name}.lib")
    } else {
        format!("lib{base_name}.a")
    }
}

pub fn archive_static(objects: &[PathBuf], output: &Path, target_triple: Option<&str>) -> PathBuf {
    if objects.is_empty() {
        cli::exit_with_error("No object files to archive");
    }

    let triple = target_triple.unwrap_or_default();

    if triple.contains("msvc") {
        archive_msvc(objects, output);
    } else {
        archive_unix(objects, output);
    }

    output.to_path_buf()
}

fn archive_unix(objects: &[PathBuf], output: &Path) {
    let candidates = ["llvm-ar", "ar"];

    for tool in candidates {
        let status = Command::new(tool)
            .arg("crus")
            .arg(output)
            .args(objects)
            .status();

        match status {
            Ok(status) if status.success() => return,
            Ok(_) => cli::exit_with_error(format!("Archiver `{tool}` failed")),
            Err(_) => continue,
        }
    }

    cli::exit_with_error("No archiver found (tried `llvm-ar`, `ar`)");
}

fn archive_msvc(objects: &[PathBuf], output: &Path) {
    let candidates = ["llvm-lib", "lib"];

    let output_flag = format!("/OUT:{}", output.to_string_lossy());

    for tool in candidates {
        let status = Command::new(tool)
            .arg("/NOLOGO")
            .arg(&output_flag)
            .args(objects)
            .status();

        match status {
            Ok(status) if status.success() => return,
            Ok(_) => cli::exit_with_error(format!("Archiver `{tool}` failed")),
            Err(_) => continue,
        }
    }

    cli::exit_with_error("No archiver found (tried `llvm-lib`, `lib`)");
}
