use sha2::{Digest, Sha256};
use url::Url;

use crate::cli;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
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
    zora_git().join("checkouts")
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

    ensure_parent_dirs(&db_url);

    db_url
}

pub fn checkout_path(url: &Url, commit: impl AsRef<str>) -> PathBuf {
    let url_hash = url_hash(url);
    let checkout_path = zora_checkouts().join(url_hash).join(commit.as_ref());

    ensure_parent_dirs(&checkout_path);

    checkout_path
}

// ==== git ====

pub fn ls_remote_branches(url: &Url) -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-remote", "--heads", url.as_str()])
        .output()
        .unwrap_or_else(|err| cli::exit_with_error(format!("Git execution error: {err}")));

    if !output.status.success() {
        cli::exit_with_error(format!(
            "Url `{url}` returned error:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .filter_map(|line| {
            let (_, ref_path) = line.split_once("\t").unwrap_or(("", ""));
            ref_path.strip_prefix("refs/heads/").map(String::from)
        })
        .collect()
}

pub fn probe_branch(url: &Url, preferred: Option<String>) -> String {
    const MAIN_BRANCHES: &[&str] = &["main", "master"];

    let remote_branches = ls_remote_branches(url);

    if let Some(preferred) = preferred {
        if !remote_branches.contains(&preferred) {
            cli::exit_with_error(format!(
                "Url `{url}` doesn't have \"{}\" branch. Available: {}",
                preferred,
                remote_branches.join(", ")
            ));
        }

        return preferred;
    }

    let available_mains = MAIN_BRANCHES
        .iter()
        .filter(|&&branch| remote_branches.contains(&branch.to_string()))
        .collect::<Vec<_>>();

    let Some(&&branch) = available_mains.first() else {
        cli::exit_with_error(format!(
            "Url `{url}`, no main branch found, please specify it in manifest"
        ));
    };

    branch.to_string()
}

pub fn ensure_bare(url: &Url) -> PathBuf {
    let db = db_path(url);
    let head = fs::read_to_string(db.join("HEAD"))
        .map(|content| content.starts_with("ref"))
        .ok();

    if let Some(head) = head
        && head
    {
        let fetch_output = Command::new("git")
            .arg("fetch")
            .arg("--quiet")
            .current_dir(&db)
            .output();

        return match fetch_output {
            Ok(out) => {
                if !out.status.success() {
                    cli::exit_with_error(format!(
                        "Git fetch returned an error:\n{}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }

                db
            }

            Err(err) => {
                cli::exit_with_error(format!("Error appeared on `git` bare fetch exec: {err}"))
            }
        };
    }

    ensure_dir(&db);

    let output = Command::new("git")
        .arg("clone")
        .args(["--bare", "--quiet"])
        .arg(url.as_str())
        .arg(&db)
        .output();

    match output {
        Ok(out) => {
            if !out.status.success() {
                cli::exit_with_error(format!(
                    "Bare git clone returned an error:\n{}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }

            db
        }
        Err(err) => cli::exit_with_error(format!("Error appeared on `git` bare clone exec: {err}")),
    }
}

pub fn rev_parse(db: &Path, rev: impl AsRef<str>) -> String {
    const GIT_HASH_LEN: usize = 40;

    let mut git_cmd = Command::new("git");

    git_cmd
        .arg(format!("--git-dir={}", db.to_string_lossy()))
        .arg("rev-parse")
        .arg("--verify")
        .arg(rev.as_ref());

    let cmd_output = git_cmd.output().unwrap_or_else(|err| {
        cli::exit_with_error(format!("Error appeared on `git` rev-parse exec: {err}"))
    });

    if !cmd_output.status.success() {
        cli::exit_with_error(format!(
            "Git rev-parse on db [{}] returned an error:\n{}",
            db.to_string_lossy(),
            String::from_utf8_lossy(&cmd_output.stderr),
        ));
    }

    let rev_hash = String::from_utf8_lossy(&cmd_output.stdout);
    let rev_hash = rev_hash.trim();

    if !(rev_hash.len() == GIT_HASH_LEN && rev_hash.chars().all(|ch| ch.is_ascii_hexdigit())) {
        cli::exit_with_error(format!(
            "Git rev-parse on db [{}] returned hash that isn't 40 characters:\n{}",
            db.to_string_lossy(),
            rev_hash
        ))
    }

    rev_hash.to_string()
}

pub fn ensure_checkout(db: &Path, commit: impl AsRef<str>, dest: &Path) {
    if !dest.is_dir() {
        return ensure_checkout_clone(db, commit, dest);
    }

    let rev_parse = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .unwrap_or_else(|err| {
            cli::exit_with_error(format!("Error appeared on `git` rev-parse exec: {err}"))
        });

    if !rev_parse.status.success() {
        cli::exit_with_error(format!(
            "Git rev-parse on dest [{}] returned an error:\n{}",
            dest.to_string_lossy(),
            String::from_utf8_lossy(&rev_parse.stderr),
        ));
    }

    let rev_hash = String::from_utf8_lossy(&rev_parse.stdout);
    let rev_hash = rev_hash.trim();

    if rev_hash == commit.as_ref() {
        return;
    }

    let checkout = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("checkout")
        .arg("--quiet")
        .arg(commit.as_ref())
        .output()
        .unwrap_or_else(|err| {
            cli::exit_with_error(format!("Error appeared on `git` checkout exec: {err}"))
        });

    if !checkout.status.success() {
        cli::exit_with_error(format!(
            "Git checkout on dest [{}] returned an error:\n{}",
            dest.to_string_lossy(),
            String::from_utf8_lossy(&checkout.stderr),
        ));
    }
}

fn ensure_checkout_clone(db: &Path, commit: impl AsRef<str>, dest: &Path) {
    let clone_cmd = Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg(db)
        .arg(dest)
        .output()
        .unwrap_or_else(|err| {
            cli::exit_with_error(format!("Error appeared on `git` clone exec: {err}"))
        });

    if !clone_cmd.status.success() {
        cli::exit_with_error(format!(
            "Git clone from db [{}] to dest [{}] returned an error:\n{}",
            db.to_string_lossy(),
            dest.to_string_lossy(),
            String::from_utf8_lossy(&clone_cmd.stderr),
        ));
    }

    let checkout_cmd = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("checkout")
        .arg("--quiet")
        .arg(commit.as_ref())
        .output()
        .unwrap_or_else(|err| {
            cli::exit_with_error(format!(
                "Error appeared on `git` cloned checkout exec: {err}"
            ))
        });

    if !checkout_cmd.status.success() {
        cli::exit_with_error(format!(
            "Git cloned checkout from db [{}] to dest [{}] returned an error:\n{}",
            db.to_string_lossy(),
            dest.to_string_lossy(),
            String::from_utf8_lossy(&checkout_cmd.stderr),
        ));
    }
}

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
