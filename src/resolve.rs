use std::collections::HashMap;

use url::Url;

use crate::{
    cli, git,
    lockfile::{self, LockDependency, LockPackage, Lockfile},
    manifest::{Dependency, Manifest},
};

pub fn resolve(manifest: &Manifest) -> Lockfile {
    resolve_with(manifest, &[], None)
}

pub fn resolve_reusing(manifest: &Manifest, previous: &Lockfile) -> Lockfile {
    if previous.version != lockfile::SUPPORTED_LOCK_VERSION {
        return resolve(manifest);
    }

    resolve_with(manifest, &previous.package, None)
}

pub fn resolve_reusing_except(manifest: &Manifest, previous: &Lockfile, refresh: &str) -> Lockfile {
    if previous.version != lockfile::SUPPORTED_LOCK_VERSION {
        return resolve(manifest);
    }

    resolve_with(manifest, &previous.package, Some(refresh))
}

fn resolve_with(manifest: &Manifest, previous: &[LockPackage], refresh: Option<&str>) -> Lockfile {
    let pinned = previous
        .iter()
        .skip(1)
        .map(|package| (package.name.clone(), package.clone()))
        .collect();

    let mut resolver = Resolver {
        nodes: Vec::new(),
        index: HashMap::new(),
        stack: Vec::new(),
        pinned,
        refresh: refresh.map(str::to_owned),
    };

    let mut refs = Vec::with_capacity(manifest.dependencies.len());

    for (name, dependency) in sorted_dependencies(&manifest.dependencies) {
        let (url, branch) = dependency_parts(dependency);
        let sha = resolver.visit(name, url, branch);

        refs.push(LockDependency {
            package: name.clone(),
            rev: sha,
        });
    }

    let mut root = LockPackage::from_manifest(manifest);
    root.dependencies = refs;

    resolver
        .nodes
        .sort_by(|left, right| left.name.cmp(&right.name));

    let mut packages = Vec::with_capacity(resolver.nodes.len() + 1);
    packages.push(root);
    packages.extend(resolver.nodes);

    Lockfile {
        version: lockfile::SUPPORTED_LOCK_VERSION,
        package: packages,
    }
}

struct Resolver {
    nodes: Vec<LockPackage>,
    index: HashMap<String, usize>,
    stack: Vec<String>,
    pinned: HashMap<String, LockPackage>,
    refresh: Option<String>,
}

impl Resolver {
    fn visit(&mut self, key: &str, url: &Url, branch: Option<String>) -> String {
        if let Some(position) = self.stack.iter().position(|name| name == key) {
            let mut cycle = self.stack[position..].to_vec();
            cycle.push(key.to_owned());

            cli::exit_with_error(format!("Dependency cycle detected: {}", cycle.join(" -> ")));
        }

        let reusable = self.refresh.as_deref() != Some(key)
            && self
                .pinned
                .get(key)
                .is_some_and(|pinned| pinned_matches(pinned, url, &branch));

        if reusable {
            let pinned = self
                .pinned
                .get(key)
                .cloned()
                .unwrap_or_else(|| cli::exit_with_error("Pinned dependency vanished"));

            if let Some(&node_index) = self.index.get(key) {
                return self.nodes[node_index].commit.clone().unwrap_or_default();
            }

            self.reuse_node(key, &pinned);

            return pinned.commit.clone().unwrap_or_default();
        }

        let probed = git::probe_branch(url, branch.clone());
        let db = git::ensure_bare(url);
        let sha = git::rev_parse(&db, &probed);

        if let Some(&node_index) = self.index.get(key) {
            let stored = &self.nodes[node_index];

            let same_source = stored
                .repository
                .as_ref()
                .is_some_and(|repository| repository == url);
            let same_commit = stored.commit.as_deref() == Some(sha.as_str());

            if !(same_source && same_commit) {
                cli::exit_with_error(format!(
                    "Dependency `{key}` resolves to different revisions"
                ));
            }

            return sha;
        }

        let dest = git::checkout_path(url, &sha);
        let fresh = !dest.exists();
        git::ensure_checkout(&db, &sha, &dest);

        if fresh {
            cli::println_info(
                "Checkout",
                format!("installed: `{key}@{}`", sha.get(..9).unwrap_or(&sha)),
            );
        }

        let dep_manifest = Manifest::load_from(&dest).unwrap_or_else(|err| {
            cli::exit_with_error(format!("Dependency `{key}` manifest error:\n{err}"));
        });

        self.stack.push(key.to_owned());

        let mut refs = Vec::with_capacity(dep_manifest.dependencies.len());

        for (child_name, child_dependency) in sorted_dependencies(&dep_manifest.dependencies) {
            let (child_url, child_branch) = dependency_parts(child_dependency);
            let child_sha = self.visit(child_name, child_url, child_branch);

            refs.push(LockDependency {
                package: child_name.clone(),
                rev: child_sha,
            });
        }

        self.stack.pop();

        let (link, link_dir) = dep_manifest
            .build
            .as_ref()
            .map(|build| (build.link_libs().to_vec(), build.link_dirs().to_vec()))
            .unwrap_or_default();

        let node = LockPackage {
            name: key.to_owned(),
            version: dep_manifest.package.version,
            zeen: dep_manifest.package.zeen,
            dependencies: refs,
            repository: Some(url.clone()),
            branch: (probed != sha).then(|| probed.clone()),
            commit: Some(sha.clone()),
            link,
            link_dir,
        };

        self.index.insert(key.to_owned(), self.nodes.len());
        self.nodes.push(node);

        sha
    }

    fn reuse_node(&mut self, key: &str, pinned: &LockPackage) {
        if self.index.contains_key(key) {
            return;
        }

        self.index.insert(key.to_owned(), self.nodes.len());

        let children = pinned.dependencies.clone();
        self.nodes.push(pinned.clone());

        for child in children {
            let Some(child_pinned) = self.pinned.get(&child.package).cloned() else {
                continue;
            };

            self.reuse_node(&child.package, &child_pinned);
        }
    }
}

fn pinned_matches(pinned: &LockPackage, url: &Url, branch: &Option<String>) -> bool {
    if pinned.repository.as_ref() != Some(url) {
        return false;
    }

    match branch {
        None => pinned.branch.is_some(),
        Some(rev) => {
            let is_sha = rev.len() == 40 && rev.chars().all(|ch| ch.is_ascii_hexdigit());

            if is_sha {
                pinned.commit.as_deref() == Some(rev.as_str())
            } else {
                pinned.branch.as_deref() == Some(rev.as_str())
            }
        }
    }
}

fn sorted_dependencies(dependencies: &HashMap<String, Dependency>) -> Vec<(&String, &Dependency)> {
    let mut sorted: Vec<(&String, &Dependency)> = dependencies.iter().collect();
    sorted.sort_by(|left, right| left.0.cmp(right.0));

    sorted
}

fn dependency_parts(dependency: &Dependency) -> (&url::Url, Option<String>) {
    match dependency {
        Dependency::Short(url) => (url, None),
        Dependency::Detailed { git, rev } => (git, rev.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::{Version, VersionReq};

    fn pinned(repository: &str, branch: Option<&str>, commit: Option<&str>) -> LockPackage {
        LockPackage {
            name: "dep".to_owned(),
            version: Version::new(0, 1, 0),
            zeen: VersionReq::parse("*").unwrap(),
            dependencies: vec![],
            repository: Some(Url::parse(repository).unwrap()),
            branch: branch.map(str::to_owned),
            commit: commit.map(str::to_owned),
            link: vec![],
            link_dir: vec![],
        }
    }

    #[test]
    fn pinned_matches_default_branch() {
        let url = Url::parse("https://github.com/mealet/zora").unwrap();
        let locked = pinned("https://github.com/mealet/zora", Some("main"), Some("abc"));

        assert!(pinned_matches(&locked, &url, &None));
    }

    #[test]
    fn pinned_rejects_unpinned_default_branch() {
        let url = Url::parse("https://github.com/mealet/zora").unwrap();
        let locked = pinned("https://github.com/mealet/zora", None, Some("abc"));

        assert!(!pinned_matches(&locked, &url, &None));
    }

    #[test]
    fn pinned_matches_named_branch() {
        let url = Url::parse("https://github.com/mealet/zora").unwrap();
        let locked = pinned("https://github.com/mealet/zora", Some("dev"), Some("abc"));

        assert!(pinned_matches(&locked, &url, &Some("dev".to_owned())));
        assert!(!pinned_matches(&locked, &url, &Some("main".to_owned())));
    }

    #[test]
    fn pinned_matches_commit_hash() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let url = Url::parse("https://github.com/mealet/zora").unwrap();
        let locked = pinned("https://github.com/mealet/zora", None, Some(hash));

        assert!(pinned_matches(&locked, &url, &Some(hash.to_owned())));
    }

    #[test]
    fn pinned_rejects_different_repository() {
        let url = Url::parse("https://github.com/other/repo").unwrap();
        let locked = pinned("https://github.com/mealet/zora", Some("main"), Some("abc"));

        assert!(!pinned_matches(&locked, &url, &None));
    }
}
