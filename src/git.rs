use sha2::{Digest, Sha256};
use url::Url;

use crate::cli;
use std::{
    fs,
    path::{Path, PathBuf},
};

// ==== dirs ====

pub fn zora_home() -> PathBuf {
    match home::home_dir() {
        Some(pathbuf) => pathbuf.join(".zora"),
        None => {
            cli::exit_with_error("Home directory is not detected for current platform.");
        }
    }
}

fn zora_git() -> PathBuf {
    zora_home().join("git")
}

fn zora_db() -> PathBuf {
    zora_git().join("db")
}

fn zora_checkouts() -> PathBuf {
    zora_home().join("checkouts")
}

pub fn url_hash(url: &Url) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_str());

    let full = hasher.finalize();

    hex::encode(&full[..16])
}

pub fn db_path(url: &Url) -> PathBuf {
    let url_hash = url_hash(url);
    let db_url = zora_db().join(url_hash);

    ensure_dir(&db_url);

    db_url
}

pub fn checkout_path(url: &Url, commit: impl AsRef<str>) -> PathBuf {
    let url_hash = url_hash(url);
    let checkout_path = zora_checkouts().join(url_hash).join(commit.as_ref());

    ensure_dir(&checkout_path);

    checkout_path
}

// ==== git ====

// ==== helpers ====

fn ensure_dir(path: &Path) {
    ensure_parent_dirs(path);

    fs::create_dir_all(path)
        .unwrap_or_else(|err| cli::exit_with_error(format!("Dirs creation returned error: {err}")));
}

fn ensure_parent_dirs(file_path: &Path) {
    match file_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent)
            .unwrap_or_else(|err| {
                cli::exit_with_error(format!("Dirs creation returned error: {err}"))
            }),
        _ => {}
    }
}
