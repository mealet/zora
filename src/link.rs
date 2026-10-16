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
        archive_msvc(objects, output, triple);
    } else {
        archive_unix(objects, output, triple);
    }

    output.to_path_buf()
}

fn archive_unix(objects: &[PathBuf], output: &Path, triple: &str) {
    let candidates = tool_candidates("AR", "ar", &["llvm-ar"], triple);

    let archived = run_first(&candidates, |command| {
        command.arg("crus").arg(output).args(objects);
    });

    if archived.is_none() {
        cli::exit_with_error(format!(
            "No archiver found (tried `{}`)",
            candidates.join("`, `")
        ));
    }
}

fn archive_msvc(objects: &[PathBuf], output: &Path, triple: &str) {
    let candidates = tool_candidates("AR", "lib", &["llvm-lib"], triple);
    let output_flag = format!("/OUT:{}", output.to_string_lossy());

    let archived = run_first(&candidates, |command| {
        command.arg("/NOLOGO").arg(&output_flag).args(objects);
    });

    if archived.is_none() {
        cli::exit_with_error(format!(
            "No archiver found (tried `{}`)",
            candidates.join("`, `")
        ));
    }
}

pub fn dynamic_library_name(base_name: &str, target_triple: Option<&str>) -> String {
    let triple = target_triple.unwrap_or_default();

    if triple.contains("windows") {
        format!("{base_name}.dll")
    } else if triple.contains("apple") || triple.contains("darwin") {
        format!("lib{base_name}.dylib")
    } else {
        format!("lib{base_name}.so")
    }
}

pub fn link_dynamic(objects: &[PathBuf], output: &Path, target_triple: Option<&str>) -> PathBuf {
    if objects.is_empty() {
        cli::exit_with_error("No object files to link");
    }

    let triple = target_triple.unwrap_or_default();

    if triple.contains("wasm") {
        cli::exit_with_error("Dynamic libraries are not supported for wasm targets");
    }

    if triple.contains("msvc") {
        link_msvc_dll(objects, output);
    } else {
        link_shared(objects, output, triple);
    }

    output.to_path_buf()
}

fn link_shared(objects: &[PathBuf], output: &Path, triple: &str) {
    let candidates = tool_candidates("CC", "cc", &["clang", "gcc"], triple);

    let shared_flag = if triple.contains("apple") || triple.contains("darwin") {
        "-dynamiclib"
    } else {
        "-shared"
    };

    let linked = run_first(&candidates, |command| {
        command.arg(shared_flag).arg("-o").arg(output).args(objects);
    });

    if linked.is_none() {
        cli::exit_with_error(format!(
            "No C compiler found (tried `{}`)",
            candidates.join("`, `")
        ));
    }
}

fn link_msvc_dll(objects: &[PathBuf], output: &Path) {
    let candidates = tool_candidates("LD", "link", &["lld-link"], "");
    let output_flag = format!("/OUT:{}", output.to_string_lossy());

    let linked = run_first(&candidates, |command| {
        command
            .arg("/NOLOGO")
            .arg("/DLL")
            .arg(&output_flag)
            .args(objects);
    });

    if linked.is_none() {
        cli::exit_with_error(format!(
            "No linker found (tried `{}`)",
            candidates.join("`, `")
        ));
    }
}

fn tool_candidates(env_key: &str, tool: &str, alternatives: &[&str], triple: &str) -> Vec<String> {
    if let Some(value) = std::env::var_os(env_key) {
        return vec![value.to_string_lossy().into_owned()];
    }

    let mut candidates = Vec::new();

    if triple.contains("gnu") || triple.contains("musl") {
        if triple.contains("windows")
            && let Some(arch) = triple.split('-').next()
        {
            candidates.push(format!("{arch}-w64-mingw32-{tool}"));
        }

        candidates.push(format!("{triple}-{tool}"));
    }

    candidates.push(tool.to_owned());
    candidates.extend(
        alternatives
            .iter()
            .map(|alternative| (*alternative).to_owned()),
    );

    candidates
}

fn run_first(candidates: &[String], configure: impl Fn(&mut Command)) -> Option<String> {
    for tool in candidates {
        let mut command = Command::new(tool);
        configure(&mut command);

        match command.status() {
            Ok(status) if status.success() => return Some(tool.clone()),
            Ok(_) => cli::exit_with_error(format!("Tool `{tool}` failed")),
            Err(_) => continue,
        }
    }

    None
}
