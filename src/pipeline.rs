use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::LazyLock,
};

use semver::{Version, VersionReq};

use crate::{
    cli::{self, CompilationMode},
    link,
    lockfile::{self, LockPackage, LockValidateError, Lockfile},
    manifest::{self, Manifest, PackageType},
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

const LIB_TYPE_DEFAULT: manifest::PackageType = manifest::PackageType::Static;
const INIT_VERSION_DEFAULT: &str = "0.0.0";

static ZEEN_VERSION: LazyLock<(Version, VersionReq)> = LazyLock::new(verify_zeen_installed);

/// Verify that Zeen compiler is installed
pub fn verify_zeen_installed() -> (Version, VersionReq) {
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
                let ver = Version::parse(&caps[1]).unwrap_or_else(|err| {
                    cli::exit_with_error(format!("Error parsing Zeen version: {}", err));
                });

                let ver_req =
                    VersionReq::parse(format!("^{}", &caps[1]).as_str()).unwrap_or_else(|err| {
                        cli::exit_with_error(format!("Error parsing Zeen version: {}", err));
                    });

                return (ver, ver_req);
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

        cli::exit_with_error(err);
    })
}

/// Load lockfile, if error is NotFound - return None
pub fn load_optional_lockfile(root: &Path) -> Option<Lockfile> {
    Lockfile::load(root)
        .map_err(|err| match err {
            crate::manifest::LoadError::Io(ref io_err)
                if let std::io::ErrorKind::NotFound = io_err.kind() =>
            {
                err
            }
            _ => cli::exit_with_error(format!("Lockfile error: {}", err)),
        })
        .ok()
}

fn ensure_lockfile(root: &Path, manifest: &Manifest) -> Lockfile {
    if let Some(mut lockfile) = load_optional_lockfile(root) {
        let synced = lockfile.sync_root(manifest);

        if synced {
            cli::println_info(
                "Updating",
                format!("lockfile `{}` (sync actions)", lockfile::LOCKFILE_CONFIG),
            );

            let lockfile_content = toml::to_string_pretty(&lockfile).unwrap_or_else(|err| {
                cli::exit_with_error(format!("Unable to serialize toml lockfile: {}", err))
            });

            fs::write(root.join(lockfile::LOCKFILE_CONFIG), lockfile_content).unwrap_or_else(
                |err| {
                    cli::exit_with_error(format!(
                        "Error with writing '{}': {}",
                        lockfile::LOCKFILE_CONFIG,
                        err
                    ))
                },
            );
        }

        // TODO: Dependencies resolver sync

        return lockfile;
    }

    // Lock not found, creating new

    cli::println_info(
        "Creating",
        format!("new lockfile `{}`", lockfile::LOCKFILE_CONFIG),
    );

    let fresh_lockfile = Lockfile::fresh(manifest);
    let lockfile_content = toml::to_string_pretty(&fresh_lockfile).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to serialize toml lockfile: {}", err))
    });

    fs::write(root.join(lockfile::LOCKFILE_CONFIG), lockfile_content).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Error with writing '{}': {}",
            lockfile::LOCKFILE_CONFIG,
            err
        ))
    });

    fresh_lockfile
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
            zeen: ZEEN_VERSION.1.to_owned(),
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

    // `Zora.lock` lockfile
    let _ = ensure_lockfile(root, &manifest);

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

pub fn cmd_check(manifest: &Manifest, root: &Path, avoid_execution: bool) {
    // Lockfile and Zeen version checks

    let lockfile = ensure_lockfile(root, manifest);

    if let Err(err) = lockfile.validate(&ZEEN_VERSION.0) {
        match err {
            LockValidateError::UnsupportedVersion(used) => {
                cli::println_error(format!(
                    "`{}` supported version is '{}', but found {}",
                    lockfile::LOCKFILE_CONFIG,
                    lockfile::SUPPORTED_LOCK_VERSION,
                    used
                ));

                cli::exit_with_error("Consider removing lockfile and running 'zora check'");
            }

            LockValidateError::ZeenMismatch {
                package,
                required,
                installed,
            } => cli::exit_with_error(format!(
                "Package `{}` requires Zeen '{}', but found '{}'",
                package, required, installed
            )),

            LockValidateError::EmptyGraph => {
                cli::println_error("Root graph in lockfile not found.");
                cli::exit_with_error("Consider removing file and running 'zora check'");
            }

            LockValidateError::RootHasSource => {
                cli::println_error("Root graph cannot have source (repository, branch, commit).");
                cli::exit_with_error("Consider removing file and running 'zora check'");
            }

            LockValidateError::MissingNode {
                wanted_by,
                package,
                rev,
            } => {
                cli::println_error(format!(
                    "Package `{wanted_by}` wanted `{package}` dependency, but it's missing."
                ));
                cli::println_error(format!("Missing graph: `{package}@{rev}`"));
                cli::exit_with_error("Consider removing file and running 'zora check'");
            }

            LockValidateError::DuplicateNode { package, rev } => {
                cli::println_error(format!(
                    "Found duplicate node of `{package}` package (`{package}@{rev}`)."
                ));
                cli::exit_with_error("Consider removing file and running 'zora check'");
            }

            LockValidateError::IncompleteNode(node) => {
                cli::println_error(format!(
                    "Lock node `{node}` is incomplete (repository/branch/commit corrupted)."
                ));
                cli::exit_with_error("Consider removing file and running 'zora check'");
            }
        }

        // In case if I forget to exit process on error
        unreachable!()
    }

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

    // Zeen sources check (via `--check` flag)

    if avoid_execution {
        return;
    }

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

type OutputPath = PathBuf;
type BuildName = String;

pub fn cmd_build(
    args: cli::BuildArgs,
    manifest: &Manifest,
    root: &Path,
) -> (OutputPath, BuildName) {
    cmd_check(manifest, root, true);

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
        (manifest.package.name.trim().to_string(), vec![])
    };
    let output_path = output_dir.join(&output_name);

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

            let success = compiler_cmd.status().map(|x| x.success()).unwrap_or(false);

            if !success {
                std::process::exit(1);
            }

            (output_dir, output_name)
        }
        PackageType::Static => {
            let triple = args.target.as_deref();

            if let Some(emit) = args.emit {
                let mut emit_command = compiler_command(&src_dir);

                emit_command
                    .arg(TYPE_LIB_ENTRY)
                    .arg(&output_path)
                    .arg("--mode")
                    .arg("Release")
                    .arg("--emit")
                    .arg(emit.to_string());

                if let Some(triple) = triple {
                    emit_command.arg("--target").arg(triple);
                }

                let success = emit_command
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if !success {
                    std::process::exit(1);
                }

                cli::println_info("Emitted", format!("{}", output_path.to_string_lossy()));

                return (output_dir, output_name);
            }

            compiler_cmd
                .arg(TYPE_LIB_ENTRY)
                .arg(&output_path)
                .args(compiler_flags)
                .args(["--mode", "Release"])
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

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

            (output_dir, output_name)
        }
        PackageType::Dynamic => {
            cli::exit_with_error("Dynamic library compilation is not implemented")
        }
    }
}

pub fn cmd_run(args: cli::RunArgs, manifest: &Manifest, root: &Path) {
    if manifest.package.r#type != PackageType::Bin {
        cli::exit_with_error("Run command isn't supported for library packages");
    }

    let (output_dir, build_name) = cmd_build(
        cli::BuildArgs {
            mode: args.mode,
            target: None,
            emit: None,
        },
        manifest,
        root,
    );

    let cmd = Command::new(format!("./{build_name}"))
        .current_dir(output_dir)
        .status()
        .unwrap_or_else(|err| cli::exit_with_error(format!("Build execution failed: {}", err)));

    std::process::exit(cmd.code().unwrap_or(1));
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
