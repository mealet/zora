use std::{fs, io::ErrorKind, path::Path};
use sha2::{Digest, Sha256};

use crate::{cli, fsio, lockfile::LOCKFILE_CONFIG, manifest::MANIFEST_CONFIG};
use super::SOURCE_DIRECTORY;

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
