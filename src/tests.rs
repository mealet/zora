use std::path::{Path, PathBuf};

use crate::cli;

pub struct TestFile(pub String);
pub struct TestModule(pub String);
pub struct TestName(pub String);

pub struct DiscoveredTest {
    pub file: TestFile,
    pub module: TestModule,
    pub name: TestName,
    pub should_fail: bool,
}

pub fn discover_tests(root: &Path) -> Vec<DiscoveredTest> {
    let tests_dir = root.join("tests");

    if !tests_dir.is_dir() {
        return Vec::new();
    }

    let mut entries: Vec<PathBuf> = Vec::new();

    let Ok(dir) = std::fs::read_dir(&tests_dir) else {
        cli::exit_with_error(format!("Unable to read `{}`", tests_dir.to_string_lossy()));
    };

    for entry in dir.flatten() {
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) == Some("zn") {
            entries.push(path);
        }
    }

    entries.sort();

    let mut tests = Vec::new();

    for path in entries {
        scan_file(&path, &mut tests);
    }

    tests
}

fn scan_file(path: &Path, tests: &mut Vec<DiscoveredTest>) {
    let content = std::fs::read_to_string(path).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Unable to read `{}`: {err}",
            path.to_string_lossy()
        ))
    });

    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let module = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut marked = false;
    let mut should_fail = false;

    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let lowered = trimmed.to_lowercase();

        if lowered == "/// @test" {
            marked = true;
            continue;
        }

        if lowered == "/// @should-fail" {
            should_fail = true;
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        let is_fn = trimmed.starts_with("pub fn") || trimmed.starts_with("fn");

        if !is_fn {
            if marked {
                cli::exit_with_error(format!(
                    "`{}`:{}: `/// @test` is not followed by a function",
                    path.to_string_lossy(),
                    index + 1
                ));
            }

            should_fail = false;
            continue;
        }

        if marked && !trimmed.starts_with("pub fn") {
            cli::exit_with_error(format!(
                "`{}`:{}: test function must be `pub`",
                path.to_string_lossy(),
                index + 1
            ));
        }

        let Some(fn_name) = fn_name_of(trimmed) else {
            cli::exit_with_error(format!(
                "`{}`:{}: cannot parse test function",
                path.to_string_lossy(),
                index + 1
            ));
        };

        if !marked {
            should_fail = false;
            continue;
        }

        tests.push(DiscoveredTest {
            file: file.clone(),
            module: module.clone(),
            name: fn_name,
            should_fail,
        });

        marked = false;
        should_fail = false;
    }

    if marked {
        cli::exit_with_error(format!(
            "`{}`: trailing `/// @test` without a function",
            path.to_string_lossy()
        ));
    }
}

fn fn_name_of(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("pub fn")
        .or_else(|| line.strip_prefix("fn"))?;

    let (name, params) = rest.split_once('(')?;

    if name.trim().is_empty() {
        return None;
    }

    let params = params.split_once(')').map(|(params, _)| params)?;

    if !params.trim().is_empty() {
        cli::exit_with_error(format!(
            "Test `{}` takes arguments, tests should take none",
            name.trim()
        ));
    }

    Some(name.trim().to_owned())
}
