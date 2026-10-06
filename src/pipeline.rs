use std::path::PathBuf;

use crate::{
    cli,
    manifest::{self, Manifest},
};

/// Layer to load manifest, or display an error if not found
pub fn load_manifest_or_exit() -> (Manifest, PathBuf) {
    Manifest::load().unwrap_or_else(|err| {
        use manifest::LoadError::*;

        let error_display = match err {
            Io(io_error) => io_error.to_string(),
            Toml(toml_error) => toml_error.to_string(),
        };

        cli::println_error(error_display);

        std::process::exit(1);
    })
}
