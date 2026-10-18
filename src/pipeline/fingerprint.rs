use sha2::{Digest, Sha256};
use std::{fs, io::ErrorKind, path::Path};

use super::SOURCE_DIRECTORY;
use crate::{cli, fsio, lockfile::LOCKFILE_CONFIG, manifest::MANIFEST_CONFIG};

pub const FINGERPRINT_FILE: &str = ".zora-fingerprint";
const FINGERPRINT_SCHEMA: &str = "zora-fingerprint-1";

pub struct BuildInputs<'a> {
    pub compiler_version: &'a str,
    pub mode: &'a str,
    pub target: Option<&'a str>,
    pub emit: Option<&'a str>,
}

pub fn compute(root: &Path, inputs: &BuildInputs<'_>) -> String {
    let mut hasher = Sha256::new();

    feed(&mut hasher, "schema", FINGERPRINT_SCHEMA.as_bytes());
    feed(&mut hasher, "compiler", inputs.compiler_version.as_bytes());
    feed(&mut hasher, "mode", inputs.mode.as_bytes());
    feed(
        &mut hasher,
        "target",
        inputs.target.unwrap_or("host").as_bytes(),
    );
    feed(
        &mut hasher,
        "emit",
        inputs.emit.unwrap_or("none").as_bytes(),
    );

    feed_file(&mut hasher, "manifest", &root.join(MANIFEST_CONFIG));
    feed_file(&mut hasher, "lockfile", &root.join(LOCKFILE_CONFIG));

    for (path, contents) in source_files(&root.join(SOURCE_DIRECTORY)) {
        feed(
            &mut hasher,
            &format!("{SOURCE_DIRECTORY}/{path}"),
            &contents,
        );
    }

    hex::encode(hasher.finalize())
}

pub fn is_current(fingerprint_path: &Path, fingerprint: &str, artifact: &Path) -> bool {
    if !artifact.is_file() {
        return false;
    }

    matches!(
        fs::read_to_string(fingerprint_path),
        Ok(stored) if stored.trim() == fingerprint
    )
}

pub fn store(fingerprint_path: &Path, fingerprint: &str) {
    fsio::write(fingerprint_path, format!("{fingerprint}\n"));
}

fn feed(hasher: &mut Sha256, label: &str, contents: &[u8]) {
    hasher.update((label.len() as u64).to_le_bytes());
    hasher.update(label.as_bytes());
    hasher.update((contents.len() as u64).to_le_bytes());
    hasher.update(contents);
}

fn feed_file(hasher: &mut Sha256, label: &str, path: &Path) {
    match fs::read(path) {
        Ok(contents) => feed(hasher, label, &contents),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            feed(hasher, &format!("{label}:missing"), &[]);
        }
        Err(err) => cli::exit_with_error(format!(
            "Unable to read `{}` for build fingerprint: {err}",
            path.to_string_lossy()
        )),
    }
}

fn source_files(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();

    collect_source_files(directory, directory, &mut files);

    files.sort_by(|left, right| left.0.cmp(&right.0));

    files
}

fn collect_source_files(directory: &Path, root: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    let entries = fs::read_dir(directory).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Unable to read `{}` for build fingerprint: {err}",
            directory.to_string_lossy()
        ))
    });

    for entry in entries {
        let entry = entry.unwrap_or_else(|err| {
            cli::exit_with_error(format!(
                "Unable to list `{}` for build fingerprint: {err}",
                directory.to_string_lossy()
            ))
        });

        let path = entry.path();

        if path.is_dir() {
            collect_source_files(&path, root, files);

            continue;
        }

        if !path.is_file() {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let contents = read_source(&path);

        files.push((relative, contents));
    }
}

fn read_source(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Unable to read `{}` for build fingerprint: {err}",
            path.to_string_lossy()
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn workspace(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("zora-fingerprint-{name}-{}", std::process::id()));

        let _ = fs::remove_dir_all(&path);

        fs::create_dir_all(path.join(SOURCE_DIRECTORY)).unwrap();

        fs::write(
            path.join(MANIFEST_CONFIG),
            "[package]\nname = \"app\"\n\n[build]\noutput = \"app\"\n",
        )
        .unwrap();

        fs::write(path.join(LOCKFILE_CONFIG), "version = 3\n").unwrap();

        path
    }

    fn base_inputs<'a>(version: &'a str, mode: &'a str) -> BuildInputs<'a> {
        BuildInputs {
            compiler_version: version,
            mode,
            target: None,
            emit: None,
        }
    }

    fn write_source(root: &Path, name: &str, contents: &str) {
        let path = root.join(SOURCE_DIRECTORY).join(name);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        fs::write(path, contents).unwrap();
    }

    #[test]
    fn fingerprint_is_stable_for_unchanged_sources() {
        let root = workspace("stable");

        write_source(&root, "main.zn", "fn main() {}");

        let first = compute(&root, &base_inputs("0.3.0", "Debug"));
        let second = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_eq!(first, second);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_ignores_timestamps() {
        let root = workspace("timestamps");

        write_source(&root, "main.zn", "fn main() {}");

        let first = compute(&root, &base_inputs("0.3.0", "Debug"));

        write_source(&root, "main.zn", "fn main() {}");

        let second = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_eq!(first, second);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_changes_with_source_contents() {
        let root = workspace("contents");

        write_source(&root, "main.zn", "fn main() {}");

        let first = compute(&root, &base_inputs("0.3.0", "Debug"));

        write_source(&root, "main.zn", "fn main() { other(); }");

        let second = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_ne!(first, second);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_changes_with_source_paths() {
        let root = workspace("paths");

        write_source(&root, "main.zn", "fn main() {}");

        let first = compute(&root, &base_inputs("0.3.0", "Debug"));

        fs::rename(
            root.join(SOURCE_DIRECTORY).join("main.zn"),
            root.join(SOURCE_DIRECTORY).join("entry.zn"),
        )
        .unwrap();

        let second = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_ne!(first, second);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_changes_with_nested_source() {
        let root = workspace("nested");

        write_source(&root, "main.zn", "fn main() {}");

        let first = compute(&root, &base_inputs("0.3.0", "Debug"));

        write_source(&root, "nested/other.zn", "pub fn other() {}");

        let second = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_ne!(first, second);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_changes_with_manifest_and_lockfile() {
        let root = workspace("manifests");

        write_source(&root, "main.zn", "fn main() {}");

        let first = compute(&root, &base_inputs("0.3.0", "Debug"));

        fs::write(
            root.join(MANIFEST_CONFIG),
            "[package]\nname = \"other\"\n\n[build]\noutput = \"other\"\n",
        )
        .unwrap();

        let second = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_ne!(first, second);

        fs::write(root.join(LOCKFILE_CONFIG), "version = 3\n\n").unwrap();

        let third = compute(&root, &base_inputs("0.3.0", "Debug"));

        assert_ne!(second, third);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_changes_with_build_context() {
        let root = workspace("context");

        write_source(&root, "main.zn", "fn main() {}");

        let debug = compute(&root, &base_inputs("0.3.0", "Debug"));
        let release = compute(&root, &base_inputs("0.3.0", "Release"));

        assert_ne!(debug, release);

        let newer_compiler = compute(&root, &base_inputs("0.4.0", "Debug"));

        assert_ne!(debug, newer_compiler);

        let cross = compute(
            &root,
            &BuildInputs {
                compiler_version: "0.3.0",
                mode: "Debug",
                target: Some("x86_64-pc-windows-gnu"),
                emit: None,
            },
        );

        assert_ne!(debug, cross);

        let emitted = compute(
            &root,
            &BuildInputs {
                compiler_version: "0.3.0",
                mode: "Debug",
                target: None,
                emit: Some("IR"),
            },
        );

        assert_ne!(debug, emitted);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn current_requires_matching_fingerprint_and_artifact() {
        let root = workspace("current");
        let fingerprint_path = root.join(FINGERPRINT_FILE);
        let artifact_path = root.join("app");

        store(&fingerprint_path, "abc");

        assert!(!is_current(&fingerprint_path, "abc", &artifact_path));

        fs::write(&artifact_path, "binary").unwrap();

        assert!(is_current(&fingerprint_path, "abc", &artifact_path));
        assert!(!is_current(&fingerprint_path, "def", &artifact_path));

        let _ = fs::remove_dir_all(&root);
    }
}
