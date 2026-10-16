use std::{collections::HashMap, fs, path::Path, process::Command};

use semver::Version;

use crate::{
    cli,
    manifest::{self, Manifest},
};

use super::{
    INIT_BIN_CODE, INIT_LIB_CODE, LIB_TYPE_DEFAULT, SOURCE_DIRECTORY, TYPE_BIN_ENTRY,
    TYPE_LIB_ENTRY, ZEEN_VERSION, ensure_lockfile, save_manifest,
};

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
    save_manifest(root, &manifest);

    // `Zora.lock` lockfile
    let _ = ensure_lockfile(root, &manifest);

    // entry file (`main.zn`/`index.zn`)
    let entry_file = if is_lib {
        TYPE_LIB_ENTRY
    } else {
        TYPE_BIN_ENTRY
    };
    let entry_content = if is_lib { INIT_LIB_CODE } else { INIT_BIN_CODE };

    crate::fsio::write(&root.join(SOURCE_DIRECTORY).join(entry_file), entry_content);

    // git project init
    init_git_repo(root);
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

        crate::fsio::write(&root.join(".gitignore"), gitignore_content);
    }
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
