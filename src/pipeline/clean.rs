use std::{fs, path::Path};

use crate::cli;

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

    let storage = crate::git::zora_home().join("git");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_scales_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
    }
}
