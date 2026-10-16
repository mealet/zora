use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{LazyLock, atomic::AtomicBool},
};

use semver::{Version, VersionReq};

use crate::{
    cli, git,
    lockfile::{self, LockValidateError, Lockfile},
    manifest::{self, Manifest},
    resolve,
};

mod build;
mod check;
mod clean;
mod deps;
mod init;
mod test_cmd;

pub use build::{cmd_build, cmd_run};
pub use check::cmd_check;
pub use clean::cmd_clean;
pub use deps::{cmd_add, cmd_deps_map, cmd_remove, cmd_update};
pub use init::{cmd_init, cmd_new};
pub use test_cmd::cmd_test;

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
pub fn load_manifest_or_exit(manifest_path: Option<&Path>) -> (Manifest, PathBuf) {
    let loaded = match manifest_path {
        Some(path) => Manifest::load_explicit(path),
        None => Manifest::load(),
    };

    loaded.unwrap_or_else(|err| cli::exit_with_error(err))
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
    let Some(lockfile) = load_optional_lockfile(root) else {
        cli::println_info(
            "Creating",
            format!("new lockfile `{}`", lockfile::LOCKFILE_CONFIG),
        );

        let resolved = resolve::resolve(manifest);
        save_lockfile(root, &resolved);

        return resolved;
    };

    let resolved = resolve::resolve_reusing(manifest, &lockfile);

    if !lockfiles_equal(&lockfile, &resolved) {
        cli::println_info(
            "Updating",
            format!("lockfile `{}` (sync actions)", lockfile::LOCKFILE_CONFIG),
        );

        save_lockfile(root, &resolved);
    }

    resolved
}

fn lockfiles_equal(left: &Lockfile, right: &Lockfile) -> bool {
    match (toml::to_string_pretty(left), toml::to_string_pretty(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn save_lockfile(root: &Path, lockfile: &Lockfile) {
    let content = toml::to_string_pretty(lockfile).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to serialize toml lockfile: {err}"))
    });

    crate::fsio::write(&root.join(lockfile::LOCKFILE_CONFIG), content);
}

fn save_manifest(root: &Path, manifest: &Manifest) {
    let content = toml::to_string_pretty(manifest).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Unable to serialize toml manifest: {err}"))
    });

    crate::fsio::write(&root.join(manifest::MANIFEST_CONFIG), content);
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

fn apply_pkg_args(command: &mut Command, lockfile: &Lockfile, root: &Path) {
    let Some(project) = lockfile.package.first() else {
        cli::exit_with_error("Lockfile has no root package");
    };

    command.arg("--pkg").arg(format!(
        "{}={}",
        project.name,
        root.join(SOURCE_DIRECTORY).to_string_lossy()
    ));

    for package in lockfile.package.iter().skip(1) {
        if package.name == project.name {
            cli::exit_with_error(format!(
                "Dependency `{}` shadows the project package name",
                package.name
            ));
        }

        let Some(repository) = &package.repository else {
            cli::exit_with_error(format!("Lock node `{}` has no repository", package.name));
        };

        let Some(commit) = &package.commit else {
            cli::exit_with_error(format!("Lock node `{}` has no commit", package.name));
        };

        let db = git::ensure_bare_local(repository);
        git::ensure_commit(&db, repository, commit);

        let dest = git::checkout_path(repository, commit);
        let fresh = !dest.exists();
        git::ensure_checkout(&db, commit, &dest);

        if fresh {
            cli::println_info(
                "Checkout",
                format!(
                    "installed: `{}@{}`",
                    package.name,
                    commit.get(..9).unwrap_or(commit)
                ),
            );
        }

        let src = dest.join(SOURCE_DIRECTORY);

        if !src.is_dir() {
            cli::println_warn(format!(
                "Dependency `{}` has no `src` directory, skipping imports",
                package.name
            ));

            continue;
        }

        command
            .arg("--pkg")
            .arg(format!("{}={}", package.name, src.to_string_lossy()));
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

fn valid_output_name(name: &str) -> bool {
    !(name.contains('.')
        || name.contains(' ')
        || name.contains('/')
        || name.contains('\\')
        || name.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_name_accepts_plain_name() {
        assert!(valid_output_name("tool"));
    }

    #[test]
    fn output_name_rejects_extension() {
        assert!(!valid_output_name("tool.exe"));
    }

    #[test]
    fn output_name_rejects_separators() {
        assert!(!valid_output_name("nested/tool"));
        assert!(!valid_output_name("nested\\tool"));
    }

    #[test]
    fn output_name_rejects_empty() {
        assert!(!valid_output_name(""));
    }
}
