use sha2::{Digest, Sha256};
use url::Url;

use crate::cli;
use std::{fs, path::PathBuf};

pub fn zora_home() -> PathBuf {
    let home_buf = match home::home_dir() {
        Some(pathbuf) => pathbuf.join(".zora"),
        None => {
            cli::exit_with_error("Home directory is not detected for current platform.");
        }
    };

    fs::create_dir_all(&home_buf).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Cannot create Zora home directory: {err}"));
    });

    home_buf
}

pub fn url_hash(url: &Url) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_str());

    let full = hasher.finalize();

    hex::encode(&full[..16])
}
