use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use colored::Colorize;

use crate::{cli, manifest::Manifest};

use super::{apply_pkg_args, cmd_check, compiler_command, load_optional_lockfile};

pub fn cmd_test(manifest: &Manifest, root: &Path) {
    let _ = cmd_check(manifest, root, true);

    let found = crate::tests::discover_tests(root);

    if found.is_empty() {
        cli::println_warn("No tests found in `tests/`");

        return;
    }

    let lockfile = load_optional_lockfile(root)
        .unwrap_or_else(|| cli::exit_with_error("Lockfile missing after check"));

    let work_dir = std::env::temp_dir().join(format!("zora-test-{}", std::process::id()));

    fs::create_dir_all(&work_dir).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to create test workspace: {err}"))
    });

    let mut passed = 0u32;
    let mut failed_names: Vec<String> = Vec::new();

    let total_started = std::time::Instant::now();
    let mut current_file = String::new();

    for test in &found {
        if test.file.0 != current_file {
            current_file = test.file.0.clone();

            cli::println_info("\nTests", format!("from `tests/{}`:", test.file.0));
        }

        let label = format!("{}.{}", test.module.0, test.name.0);

        let entry = format!(
            r#"
use tests.{module};

fn main() {{
    {name}();
}}
"#,
            module = test.module.0,
            name = test.name.0
        );
        let entry_path = work_dir.join(format!("{}_{}.zn", test.module.0, test.name.0));

        fs::write(&entry_path, entry).unwrap_or_else(|err| {
            cli::exit_with_error(format!("Unable to write test entry: {err}"))
        });

        let bin_path = work_dir.join(format!(
            "{}_{}{}",
            test.module.0,
            test.name.0,
            if cfg!(target_os = "windows") {
                ".exe"
            } else {
                ""
            }
        ));

        let mut compile = compiler_command(&work_dir);

        compile
            .arg(&entry_path)
            .arg(&bin_path)
            .arg("--mode")
            .arg("Debug")
            .arg("--pkg")
            .arg(format!("tests={}", root.join("tests").to_string_lossy()));

        apply_pkg_args(&mut compile, &lockfile, root);

        compile.stdout(Stdio::null()).stderr(Stdio::piped());

        let compiled = compile
            .output()
            .unwrap_or_else(|err| cli::exit_with_error(format!("Unable to run compiler: {err}")));

        if !compiled.status.success() {
            cli::println_basic(format!("  test {label} ... {}", "[COMPILE ERROR]".red()));
            cli::println_basic(String::from_utf8_lossy(&compiled.stderr).into_owned());
            std::process::exit(1);
        }

        let run = Command::new(&bin_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap_or_else(|err| cli::exit_with_error(format!("Unable to run test: {err}")));

        let code = run.status.code().unwrap_or(1);
        let ok = if test.should_fail {
            code != 0
        } else {
            code == 0
        };

        if ok {
            passed += 1;
            cli::println_basic(format!("  test {label} ... {}", "[OK]".green()));
        } else {
            cli::println_basic(format!("  test {label} ... {}", "[FAILED]".red()));
            cli::println_basic("");

            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );

            for line in output.lines() {
                cli::println_basic(format!("{} {line}", format!("[{label}]:").bright_black()));
            }

            cli::println_basic("");
            failed_names.push(label);
        }
    }

    let _ = fs::remove_dir_all(&work_dir);

    if failed_names.is_empty() {
        cli::println_info(
            "\nTests",
            format!(
                "{passed} passed in {:.2}s",
                total_started.elapsed().as_secs_f32()
            ),
        );

        cli::println_basic("");
    } else {
        cli::println_info(
            "\nTests",
            format!(
                "{passed} passed, {} failed in {:.2}s",
                failed_names.len(),
                total_started.elapsed().as_secs_f32()
            ),
        );

        for name in failed_names {
            cli::println_basic(format!("  {name}"));
        }

        cli::println_basic("");
        std::process::exit(1);
    }
}
