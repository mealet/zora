use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::cli;

pub fn write(path: &Path, contents: impl AsRef<[u8]>) {
    let temporary = temporary_path(path);

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).unwrap_or_else(|err| {
            cli::exit_with_error(format!(
                "Unable to create `{}`: {err}",
                parent.to_string_lossy()
            ))
        });
    }

    fs::write(&temporary, contents).unwrap_or_else(|err| {
        cli::exit_with_error(format!(
            "Unable to write `{}`: {err}",
            temporary.to_string_lossy()
        ))
    });

    fs::rename(&temporary, path).unwrap_or_else(|err| {
        let _ = fs::remove_file(&temporary);

        cli::exit_with_error(format!(
            "Unable to replace `{}`: {err}",
            path.to_string_lossy()
        ))
    });
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();

    name.push(format!(".{}.tmp", std::process::id()));

    path.with_file_name(name)
}
