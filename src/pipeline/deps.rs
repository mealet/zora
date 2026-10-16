use std::path::Path;

use url::Url;

use crate::{
    cli, git,
    lockfile::Lockfile,
    manifest::{self, Manifest},
    resolve,
};

use super::{
    cmd_check, dependency_sources, ensure_lockfile, load_optional_lockfile, save_lockfile,
    save_manifest, validate_lockfile_or_exit,
};

fn resolve_manifest(manifest: &Manifest, root: &Path) -> Lockfile {
    match load_optional_lockfile(root) {
        Some(previous) => resolve::resolve_reusing(manifest, &previous),
        None => resolve::resolve(manifest),
    }
}

pub fn cmd_add(args: cli::AddArgs, manifest: &Manifest, root: &Path) {
    let url = Url::parse(&args.url).unwrap_or_else(|err| {
        cli::exit_with_error(format!("Url parse error: {err}"));
    });

    let probe = git::probe_branch(&url, args.branch.clone());
    let db = git::ensure_bare(&url);
    let sha = git::rev_parse(&db, &probe);
    let dest = git::checkout_path(&url, &sha);
    let fresh_checkout = !dest.exists();

    git::ensure_checkout(&db, &sha, &dest);

    let dest_manifest_path = dest.join(manifest::MANIFEST_CONFIG);

    let dep_name = if dest_manifest_path.is_file() {
        let dest_manifest = Manifest::load_from(&dest).unwrap_or_else(|err| {
            cli::exit_with_error(format!("Dependency `{}` manifest error:\n{err}", url));
        });

        dest_manifest.package.name
    } else {
        cli::println_warn(format!(
            "Dependency doesn't have `{}`, name will be taken from repository",
            manifest::MANIFEST_CONFIG
        ));

        url.path_segments()
            .unwrap_or_else(|| {
                cli::exit_with_error(format!("Url `{}` path segments unavailable", url))
            })
            .rev()
            .find(|s| !s.is_empty())
            .map(|s| s.to_string())
            .map(|name| name.strip_suffix(".git").unwrap_or(&name).to_string())
            .unwrap_or_else(|| {
                cli::exit_with_error(format!("Url `{}` cannot extract name from url", url))
            })
    };

    if fresh_checkout {
        cli::println_info(
            "Checkout",
            format!("installed: `{dep_name}@{}`", sha.get(..9).unwrap_or(&sha)),
        );
    }

    let mut update = false;

    if let Some(existing) = manifest.dependencies.get(&dep_name) {
        let same_url = match existing {
            manifest::Dependency::Short(existing_url) => existing_url == &url,
            manifest::Dependency::Detailed { git, .. } => git == &url,
        };

        if !same_url {
            cli::exit_with_error(format!(
                "Dependency `{dep_name}` already exists with different url"
            ));
        }

        update = true;
        cli::println_info("Updating", format!("dependency `{dep_name}`"));
    }

    let mut new_manifest = manifest.clone();
    let entry = match &args.branch {
        Some(branch) => manifest::Dependency::Detailed {
            git: url.clone(),
            rev: Some(branch.clone()),
        },
        None => manifest::Dependency::Short(url.clone()),
    };

    new_manifest.dependencies.insert(dep_name.clone(), entry);

    let new_lockfile = match load_optional_lockfile(root) {
        Some(previous) => resolve::resolve_reusing_except(&new_manifest, &previous, &dep_name),
        None => resolve::resolve(&new_manifest),
    };

    save_manifest(root, &new_manifest);
    save_lockfile(root, &new_lockfile);

    if update {
        cli::println_info("Updated", format!("dependency `{}` in project", dep_name));
    } else {
        let dep_version = new_lockfile
            .package
            .iter()
            .find(|package| package.name == dep_name)
            .map(|package| package.version.to_string())
            .unwrap_or_default();

        cli::println_info(
            "Added",
            format!("dependency `{dep_name}` v{dep_version} to the project"),
        );
    }

    let _ = cmd_check(&new_manifest, root, true);
}

pub fn cmd_remove(args: cli::RemoveArgs, manifest: &Manifest, root: &Path) {
    if !manifest.dependencies.contains_key(&args.name) {
        cli::exit_with_error(format!("Dependency `{}` not found", args.name));
    }

    let mut new_manifest = manifest.clone();
    new_manifest.dependencies.remove(&args.name);

    let new_lockfile = resolve_manifest(&new_manifest, root);

    save_manifest(root, &new_manifest);
    save_lockfile(root, &new_lockfile);

    cli::println_info("Removed", format!("dependency `{}`", args.name));

    let _ = cmd_check(&new_manifest, root, true);
}

pub fn cmd_update(manifest: &Manifest, root: &Path) {
    let new_lockfile = resolve::resolve(manifest);

    save_lockfile(root, &new_lockfile);

    let _ = cmd_check(manifest, root, true);
}

pub fn cmd_deps_map(manifest: &Manifest, root: &Path) {
    let lockfile = ensure_lockfile(root, manifest);
    validate_lockfile_or_exit(&lockfile);

    for (name, src) in dependency_sources(&lockfile) {
        println!("{name}={}", src.to_string_lossy());
    }
}
