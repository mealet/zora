use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::LazyLock,
};

use semver::{Version, VersionReq};

use crate::{
    cli,
    manifest::{self, Manifest},
};

const COMPILER_COMMAND: &str = "zeen";
const AVAILABLE_BUILD_FLAGS: &[&str] = &[];

const SOURCE_DIRECTORY: &str = "src";
const TYPE_BIN_ENTRY: &str = "main.zn";
const TYPE_LIB_ENTRY: &str = "index.zn";

const INIT_BIN_CODE: &str = r#"
fn main() {
    @println("Hello, Zeen!");
    @println("2 + 2 = {}", 2 + 2);
}

"#;

const INIT_LIB_CODE: &str = r#"
pub fn add_int(a: i32, b: i32) i32 {
    a + b
}

"#;

const LIB_TYPE_DEFAULT: manifest::PackageType = manifest::PackageType::Dynamic;
const INIT_VERSION_DEFAULT: &str = "0.0.0";

static ZEEN_VERSION: LazyLock<VersionReq> = LazyLock::new(verify_zeen_installed);

/// Verify that Zeen compiler is installed
pub fn verify_zeen_installed() -> VersionReq {
    let output = Command::new(COMPILER_COMMAND)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            let re =
                regex::Regex::new(r"version:\s*(\d+\.\d+\.\d+)").expect("regex construction error");
            let caps = re.captures(&stdout);

            if let Some(caps) = caps {
                return VersionReq::parse(format!("^{}", &caps[1]).as_str()).unwrap_or_else(
                    |err| {
                        cli::exit_with_error(format!("Error parsing Zeen version: {}", err));
                    },
                );
            }

            cli::exit_with_error(
                "Unable to extract Zeen version, please report this: https://github.com/mealet/zora",
            );
        }
        _ => {
            cli::exit_with_error(
                "Zeen compiler is not found, please verify installation (check `zeen --version`)",
            );
        }
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

/// Init new git repository (if `git` available) and write `.gitignore`
fn init_git_repo(root: &Path) {
    let initialized = Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(root)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);

    if initialized {
        let mut gitignore_content = ["/dist"].join("\n");
        gitignore_content.push('\n');

        let _ = fs::write(root.join(".gitignore"), gitignore_content);
    }
}

// ==== Initialization Commands ====

/// Init project in provided dir
/// Note: No error handling, process with exit with error display
fn project_init(name: impl AsRef<str>, root: &Path, is_lib: bool) {
    if root.join(manifest::MANIFEST_CONFIG).is_file() {
        cli::exit_with_error("Project is already initialized here");
    }

    let manifest = Manifest {
        package: manifest::Package {
            name: name.as_ref().to_owned(),
            version: Version::new(0, 0, 0),
            zeen: ZEEN_VERSION.to_owned(),
            r#type: if is_lib {
                LIB_TYPE_DEFAULT
            } else {
                manifest::PackageType::Bin
            },
            authors: None,
            repository: None,
            license: None,
        },
        build: None,
        dependencies: HashMap::new(),
    };

    // `src` directory
    fs::create_dir_all(root.join(SOURCE_DIRECTORY)).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to create source directory: {}", err))
    });

    // `Zora.toml` config
    let manifest_content = toml::to_string_pretty(&manifest).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to serialize toml manifest: {}", err));
    });

    fs::write(root.join(manifest::MANIFEST_CONFIG), manifest_content).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Error with writing '{}': {}",
            manifest::MANIFEST_CONFIG,
            err
        ))
    });

    // entry file (`main.zn`/`index.zn`)
    let entry_file = if is_lib {
        TYPE_LIB_ENTRY
    } else {
        TYPE_BIN_ENTRY
    };
    let entry_content = if is_lib { INIT_LIB_CODE } else { INIT_BIN_CODE };

    fs::write(root.join(SOURCE_DIRECTORY).join(entry_file), entry_content).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error with filling entry file: {}", err))
    });

    // git project init
    init_git_repo(root);
}

pub fn cmd_init(args: &cli::InitArgs) {
    let cwd = std::env::current_dir().unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to get current working directory: {}", err));
    });

    let name = cwd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            cli::exit_with_error("Current directory is unnamed, use instead: `zora new`");
        });

    project_init(&name, &cwd, args.library);

    let init_type = if args.library { "library" } else { "binary" };
    cli::println_info(
        "Initialized",
        format!(
            "{init_type} package `{name}` (in {})",
            cwd.to_string_lossy()
        ),
    );
}

pub fn cmd_new(args: &cli::NewArgs) {
    let cwd = std::env::current_dir().unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to get current working directory: {err}"));
    });

    let root = cwd.join(&args.name);

    if root.is_dir() {
        cli::exit_with_error(format!("Dir name `{}` already taken", args.name));
    }

    fs::create_dir(&root).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to create `{}` dir: {}", args.name, err));
    });

    project_init(&args.name, &root, args.library);

    let init_type = if args.library { "library" } else { "binary" };
    cli::println_info(
        "Created",
        format!(
            "new {init_type} package `{}` (in {})",
            args.name,
            cwd.to_string_lossy()
        ),
    );
}

// ==== Manifest Required Commands ====

pub fn cmd_check(manifest: &Manifest, root: &Path) {
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

    // TODO: 3. Dependencies check and Zeen version check

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
