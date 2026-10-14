use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{LazyLock, atomic::AtomicBool},
};

use colored::Colorize;
use semver::{Version, VersionReq};
use url::Url;

use crate::{
    cli::{self, CompilationMode},
    git, link,
    lockfile::{self, LockPackage, LockValidateError, Lockfile},
    manifest::{self, Manifest, PackageType},
    resolve,
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
static NO_COLOR: AtomicBool = AtomicBool::new(false);

pub fn disable_color() {
    NO_COLOR.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Verify that Zeen compiler and git is installed
pub fn verify_zeen_installed() -> (Version, VersionReq) {
    // firstly git, zeen only after
    {
        let git_cmd = Command::new("git")
            .arg("--version")
            .output()
            .map(|x| x.status.success())
            .unwrap_or(false);

        if !git_cmd {
            cli::exit_with_error("No `git` installed found");
        }
    }

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

    if NO_COLOR.load(std::sync::atomic::Ordering::Relaxed) {
        command.arg("--no-color");
    }

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

fn apply_pkg_args(command: &mut Command, lockfile: &Lockfile) {
    for (name, src) in dependency_sources(lockfile) {
        command
            .arg("--pkg")
            .arg(format!("{name}={}", src.to_string_lossy()));
    }
}

fn dependency_sources(lockfile: &Lockfile) -> Vec<(String, PathBuf)> {
    let mut sources = Vec::new();

    for package in lockfile.package.iter().skip(1) {
        let Some(repository) = &package.repository else {
            cli::exit_with_error(format!("Lock node `{}` has no repository", package.name));
        };

        let Some(commit) = &package.commit else {
            cli::exit_with_error(format!("Lock node `{}` has no commit", package.name));
        };

        let db = git::ensure_bare_local(repository);
        git::ensure_commit(&db, repository, commit);

        let dest = git::checkout_path(repository, commit);
        git::ensure_checkout(&db, commit, &dest);

        let src = dest.join(SOURCE_DIRECTORY);

        if !src.is_dir() {
            cli::println_warn(format!(
                "Dependency `{}` has no `src` directory, skipping imports",
                package.name
            ));

            continue;
        }

        sources.push((package.name.clone(), src));
    }

    sources
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
        return lockfile;
    }

    let src_dir = root.join(SOURCE_DIRECTORY);

    let mut check_command = compiler_command(&src_dir);
    apply_pkg_args(&mut check_command, &lockfile);

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

fn validate_lockfile_or_exit(lockfile: &Lockfile) {
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
    }
}

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
        (manifest.package.name.trim().to_string(), vec![])
    };
    let output_path = output_dir.join(match manifest.package.r#type {
        PackageType::Bin if cfg!(target_os = "windows") => format!("{output_name}.exe"),
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
            apply_pkg_args(&mut compiler_cmd, &lockfile);

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

                apply_pkg_args(&mut compiler_cmd, &lockfile);

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
                .args(["--mode", "Release"])
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            apply_pkg_args(&mut compiler_cmd, &lockfile);

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
                    .arg("Release")
                    .arg("--emit")
                    .arg(emit.to_string());

                if let Some(triple) = triple {
                    emit_command.arg("--target").arg(triple);
                }

                apply_pkg_args(&mut compiler_cmd, &lockfile);

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
                .args(["--mode", "Release"])
                .args(["--emit", "OBJ"]);

            if let Some(ref target) = args.target {
                compiler_cmd.arg("--target").arg(target);
            }

            apply_pkg_args(&mut compiler_cmd, &lockfile);

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
        .status()
        .unwrap_or_else(|err| cli::exit_with_error(format!("Build execution failed: {}", err)));

    std::process::exit(cmd.code().unwrap_or(1));
}

pub fn cmd_add(args: cli::AddArgs, manifest: &Manifest, root: &Path) {
    let url = Url::parse(&args.url).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Url parse error: {err}"));
    });

    let probe = git::probe_branch(&url, args.branch.clone());
    let db = git::ensure_bare(&url);
    let sha = git::rev_parse(&db, &probe);
    let dest = git::checkout_path(&url, &sha);
    let fresh_checkout = !dest.exists();

    git::ensure_checkout(&db, &sha, &dest);

    let dest_manifest_path = dest.join(manifest::MANIFEST_CONFIG);

    let dep_name = if dest_manifest_path.is_file() {
        let dest_manifest = Manifest::load_from(&dest).unwrap_or_else(|err| {
            cli::exit_with_error(format!("Dependency `{}` manifest error:\n{err}", url));
        });

        dest_manifest.package.name
    } else {
        cli::println_warn(format!(
            "Dependency doesn't have `{}`, name will be taken from repository",
            manifest::MANIFEST_CONFIG
        ));

        url.path_segments()
            .unwrap_or_else(|| {
                cli::exit_with_error(format!("Url `{}` path segments unavailable", url))
            })
            .rev()
            .find(|s| !s.is_empty())
            .map(|s| s.to_string())
            .map(|name| name.strip_suffix(".git").unwrap_or(&name).to_string())
            .unwrap_or_else(|| {
                cli::exit_with_error(format!("Url `{}` cannot extract name from url", url))
            })
    };

    if fresh_checkout {
        cli::println_info(
            "Checkout",
            format!("installed: `{dep_name}@{}`", sha.get(..9).unwrap_or(&sha)),
        );
    }

    let mut update = false;

    if let Some(existing) = manifest.dependencies.get(&dep_name) {
        let same_url = match existing {
            manifest::Dependency::Short(existing_url) => existing_url == &url,
            manifest::Dependency::Detailed { git, .. } => git == &url,
        };

        if !same_url {
            cli::exit_with_error(format!(
                "Dependency `{dep_name}` already exists with different url"
            ));
        }

        update = true;
        cli::println_info("Updating", format!("dependency `{dep_name}`"));
    }

    let mut new_manifest = manifest.clone();
    let entry = match &args.branch {
        Some(branch) => manifest::Dependency::Detailed {
            git: url.clone(),
            rev: Some(branch.clone()),
        },
        None => manifest::Dependency::Short(url.clone()),
    };

    new_manifest.dependencies.insert(dep_name.clone(), entry);

    let new_lockfile = resolve::resolve(&new_manifest);

    fs::write(
        root.join(manifest::MANIFEST_CONFIG),
        toml::to_string_pretty(&new_manifest).unwrap_or_else(|err| {
            cli::exit_with_error(format!("New manifest serialization error: {err}"))
        }),
    )
    .unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error with writing updated manifest: {err}"));
    });

    fs::write(
        root.join(lockfile::LOCKFILE_CONFIG),
        toml::to_string_pretty(&new_lockfile).unwrap_or_else(|err| {
            cli::exit_with_error(format!("New lockfile serialization error: {err}"))
        }),
    )
    .unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error with writing updated lockfile: {err}"));
    });

    if update {
        cli::println_info("Updated", format!("dependency `{}` in project", dep_name));
    } else {
        let dep_version = new_lockfile
            .package
            .iter()
            .find(|package| package.name == dep_name)
            .map(|package| package.version.to_string())
            .unwrap_or_default();

        cli::println_info(
            "Added",
            format!("dependency `{dep_name}` v{dep_version} to the project"),
        );
    }

    let _ = cmd_check(&new_manifest, root, true);
}

pub fn cmd_remove(args: cli::RemoveArgs, manifest: &Manifest, root: &Path) {
    if !manifest.dependencies.contains_key(&args.name) {
        cli::exit_with_error(format!("Dependency `{}` not found", args.name));
    }

    let mut new_manifest = manifest.clone();
    new_manifest.dependencies.remove(&args.name);

    let new_lockfile = resolve::resolve(&new_manifest);

    fs::write(
        root.join(manifest::MANIFEST_CONFIG),
        toml::to_string_pretty(&new_manifest).unwrap_or_else(|err| {
            cli::exit_with_error(format!("New manifest serialization error: {err}"))
        }),
    )
    .unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error with writing updated manifest: {err}"));
    });

    fs::write(
        root.join(lockfile::LOCKFILE_CONFIG),
        toml::to_string_pretty(&new_lockfile).unwrap_or_else(|err| {
            cli::exit_with_error(format!("New lockfile serialization error: {err}"))
        }),
    )
    .unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error with writing updated lockfile: {err}"));
    });

    cli::println_info("Removed", format!("dependency `{}`", args.name));

    let _ = cmd_check(&new_manifest, root, true);
}

pub fn cmd_clean(args: cli::CleanArgs, root: &Path) {
    let dist = root.join("dist");

    if dist.is_dir() {
        let freed = dir_size(&dist);

        fs::remove_dir_all(&dist)
            .unwrap_or_else(|err| cli::exit_with_error(format!("Unable to remove `dist`: {err}")));

        cli::println_info(
            "Removed",
            format!("{} ({})", dist.to_string_lossy(), human_size(freed)),
        );
    }

    if !args.global {
        return;
    }

    let storage = git::zora_home().join("git");

    if storage.is_dir() {
        let freed = dir_size(&storage);

        fs::remove_dir_all(&storage).unwrap_or_else(|err| {
            cli::exit_with_error(format!("Unable to remove global storage: {err}"))
        });

        cli::println_info(
            "Removed",
            format!("{} ({})", storage.to_string_lossy(), human_size(freed)),
        );
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;

    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            total += dir_size(&entry.path());
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
        }
    }

    total
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

    let mut size = bytes as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

pub fn cmd_update(manifest: &Manifest, root: &Path) {
    let new_lockfile = resolve::resolve(manifest);

    fs::write(
        root.join(lockfile::LOCKFILE_CONFIG),
        toml::to_string_pretty(&new_lockfile).unwrap_or_else(|err| {
            cli::exit_with_error(format!("New lockfile serialization error: {err}"))
        }),
    )
    .unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error with writing updated lockfile: {err}"))
    });

    let _ = cmd_check(manifest, root, true);
}

pub fn cmd_deps_map(manifest: &Manifest, root: &Path) {
    let lockfile = ensure_lockfile(root, manifest);
    validate_lockfile_or_exit(&lockfile);

    for (name, src) in dependency_sources(&lockfile) {
        println!("{name}={}", src.to_string_lossy());
    }
}

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

    for test in &found {
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

        if !matches!(manifest.package.r#type, manifest::PackageType::Bin) {
            compile.arg("--pkg").arg(format!(
                "{}={}",
                manifest.package.name,
                root.join("src").to_string_lossy()
            ));
        }

        apply_pkg_args(&mut compile, &lockfile);

        compile.stdout(Stdio::null()).stderr(Stdio::piped());

        let compiled = compile
            .output()
            .unwrap_or_else(|err| cli::exit_with_error(format!("Unable to run compiler: {err}")));

        if !compiled.status.success() {
            cli::println_info("Testing", format!("{label} ... COMPILE ERROR"));
            cli::println_basic(String::from_utf8_lossy(&compiled.stderr).into_owned());
            std::process::exit(1);
        }

        let code = Command::new(&bin_path)
            .status()
            .map(|status| status.code().unwrap_or(1))
            .unwrap_or(1);

        let ok = if test.should_fail {
            code != 0
        } else {
            code == 0
        };

        if ok {
            passed += 1;
            cli::println_info("Testing", format!("{label} ... {}", "[OK]".green()));
        } else {
            cli::println_info("Testing", format!("{label} ... {}", "[FAILED]".red()));
            failed_names.push(label);
        }
    }

    let _ = fs::remove_dir_all(&work_dir);

    if failed_names.is_empty() {
        cli::println_info("Tests", format!("{passed} passed"));
    } else {
        cli::println_info(
            "Tests",
            format!("{passed} passed, {} failed", failed_names.len()),
        );

        for name in failed_names {
            cli::println_basic(format!("  {name}"));
        }

        std::process::exit(1);
    }
}
