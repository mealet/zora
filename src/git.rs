use sha2::{Digest, Sha256};
use url::Url;

use crate::cli;
use std::{
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

// ==== dirs ====

pub fn zora_home() -> PathBuf {
    if let Some(path) = std::env::var_os("ZORA_HOME") {
        return PathBuf::from(path);
    }

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
    hasher.update(canonical_url(url));

    let full = hasher.finalize();

    hex::encode(&full[..16])
}

fn canonical_url(url: &Url) -> String {
    let scheme = url.scheme().to_ascii_lowercase();
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();

    let mut path = url.path().to_owned();

    if let Some(stripped) = path.strip_suffix(".git") {
        path = stripped.to_owned();
    }

    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }

    format!("{scheme}://{host}{port}{path}")
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

    if let Some(preferred) = preferred {
        if preferred.len() == 40 && preferred.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return preferred;
        }

        let remote_branches = ls_remote_branches(url);

        if !remote_branches.contains(&preferred) {
            cli::exit_with_error(format!(
                "Url `{url}` doesn't have \"{}\" branch. Available: {}",
                preferred,
                remote_branches.join(", ")
            ));
        }

        return preferred;
    }

    let remote_branches = ls_remote_branches(url);

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
    let _guard = GitLock::acquire();
    let db = db_path(url);

    if is_bare_repo(&db) {
        fetch_bare(&db).unwrap_or_else(|err| cli::exit_with_error(err));

        return db;
    }

    reset_dir(&db);

    if let Err(clone_err) = clone_bare(url, &db) {
        if is_bare_repo(&db) {
            fetch_bare(&db).unwrap_or_else(|err| cli::exit_with_error(err));

            return db;
        }

        cli::exit_with_error(clone_err);
    }

    db
}

pub fn ensure_bare_local(url: &Url) -> PathBuf {
    let _guard = GitLock::acquire();
    let db = db_path(url);

    if is_bare_repo(&db) {
        return db;
    }

    reset_dir(&db);

    if let Err(clone_err) = clone_bare(url, &db) {
        if is_bare_repo(&db) {
            return db;
        }

        cli::exit_with_error(clone_err);
    }

    db
}

fn is_bare_repo(db: &Path) -> bool {
    db.join("objects").is_dir() && db.join("refs").is_dir()
}

fn reset_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).unwrap_or_else(|err| {
            cli::exit_with_error(format!(
                "Unable to remove dir [{}]: {err}",
                path.to_string_lossy()
            ))
        });
    }

    ensure_dir(path);
}

fn clone_bare(url: &Url, db: &Path) -> Result<(), String> {
    ensure_dir(db);

    let output = Command::new("git")
        .arg("clone")
        .args(["--bare", "--quiet"])
        .arg(url.as_str())
        .arg(db)
        .output()
        .map_err(|err| format!("Error appeared on `git` bare clone exec: {err}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Bare git clone returned an error:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn fetch_bare(db: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["fetch", "--quiet", "origin", "+refs/heads/*:refs/heads/*"])
        .current_dir(db)
        .output()
        .map_err(|err| format!("Error appeared on `git` bare fetch exec: {err}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Git fetch returned an error:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

pub fn ensure_commit(db: &Path, url: &Url, commit: &str) {
    let _guard = GitLock::acquire();

    let present = Command::new("git")
        .arg(format!("--git-dir={}", db.to_string_lossy()))
        .args(["cat-file", "-e", commit])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if present {
        return;
    }

    let fetch = Command::new("git")
        .arg(format!("--git-dir={}", db.to_string_lossy()))
        .args(["fetch", "--quiet", "origin"])
        .status();

    match fetch {
        Ok(status) if status.success() => {}
        _ => cli::exit_with_error(format!("Commit `{commit}` not found in `{url}`")),
    }

    let present = Command::new("git")
        .arg(format!("--git-dir={}", db.to_string_lossy()))
        .args(["cat-file", "-e", commit])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if !present {
        cli::exit_with_error(format!("Commit `{commit}` not found in `{url}`"));
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
    let _guard = GitLock::acquire();

    if let Some(head) = checkout_head(dest) {
        if head == commit.as_ref() {
            return;
        }

        checkout_commit(dest, commit.as_ref());

        return;
    }

    reset_dir(dest);

    ensure_checkout_clone(db, commit, dest);
}

fn checkout_head(dest: &Path) -> Option<String> {
    if !dest.is_dir() {
        return None;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    if hash.is_empty() { None } else { Some(hash) }
}

fn checkout_commit(dest: &Path, commit: &str) {
    let checkout = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("checkout")
        .arg("--quiet")
        .arg(commit)
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

const LOCK_STALE: Duration = Duration::from_secs(600);

struct GitLock {
    path: PathBuf,
}

impl GitLock {
    fn acquire() -> GitLock {
        let dir = zora_git();

        ensure_dir(&dir);

        let path = dir.join(".lock");

        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return GitLock { path },
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        let _ = fs::remove_file(&path);

                        continue;
                    }

                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(err) => cli::exit_with_error(format!("Unable to acquire git lock: {err}")),
            }
        }
    }
}

impl Drop for GitLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    let modified = fs::metadata(path).and_then(|meta| meta.modified());

    let Ok(modified) = modified else {
        return false;
    };

    SystemTime::now()
        .duration_since(modified)
        .map(|age| age > LOCK_STALE)
        .unwrap_or(false)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn canonical_url_ignores_git_suffix_and_trailing_slash() {
        let base = canonical_url(&repo("https://github.com/mealet/zora"));
        let suffix = canonical_url(&repo("https://github.com/mealet/zora.git"));
        let slash = canonical_url(&repo("https://github.com/mealet/zora/"));

        assert_eq!(base, suffix);
        assert_eq!(base, slash);
    }

    #[test]
    fn canonical_url_lowercases_host() {
        let mixed = canonical_url(&repo("https://GitHub.com/mealet/zora"));
        let lower = canonical_url(&repo("https://github.com/mealet/zora"));

        assert_eq!(mixed, lower);
    }

    #[test]
    fn url_hash_matches_for_equivalent_urls() {
        let plain = url_hash(&repo("https://github.com/mealet/zora"));
        let with_suffix = url_hash(&repo("https://github.com/mealet/zora.git"));

        assert_eq!(plain, with_suffix);
    }

    #[test]
    fn probe_branch_accepts_full_commit_hash() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let probed = probe_branch(&repo("https://example.com/repo"), Some(commit.to_owned()));

        assert_eq!(probed, commit);
    }

    #[test]
    fn detects_bare_repository_dirs() {
        let base = std::env::temp_dir().join(format!("zora-git-test-bare-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);

        fs::create_dir_all(base.join("objects")).unwrap();
        fs::create_dir_all(base.join("refs")).unwrap();

        assert!(is_bare_repo(&base));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_dir_without_repo_layout() {
        let base =
            std::env::temp_dir().join(format!("zora-git-test-nonrepo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);

        fs::create_dir_all(&base).unwrap();

        assert!(!is_bare_repo(&base));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn checkout_head_missing_dir_is_none() {
        let path = std::env::temp_dir().join("zora-missing-checkout");
        let _ = fs::remove_dir_all(&path);

        assert!(checkout_head(&path).is_none());
    }
}
