use std::collections::HashMap;

use url::Url;

use crate::{
    cli, git,
    lockfile::{self, LockDependency, LockPackage, Lockfile},
    manifest::{Dependency, Manifest},
};

pub fn resolve(manifest: &Manifest) -> Lockfile {
    let mut resolver = Resolver {
        nodes: Vec::new(),
        index: HashMap::new(),
        stack: Vec::new(),
    };

    let mut refs = Vec::with_capacity(manifest.dependencies.len());

    for (name, dependency) in &manifest.dependencies {
        let (url, branch) = dependency_parts(dependency);
        let sha = resolver.visit(name, url, branch);

        refs.push(LockDependency {
            package: name.clone(),
            rev: sha,
        });
    }

    let mut root = LockPackage::from_manifest(manifest);
    root.dependencies = refs;

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
}

impl Resolver {
    fn visit(&mut self, key: &str, url: &Url, branch: Option<String>) -> String {
        if let Some(position) = self.stack.iter().position(|name| name == key) {
            let mut cycle = self.stack[position..].to_vec();
            cycle.push(key.to_owned());

            cli::exit_with_error(format!("Dependency cycle detected: {}", cycle.join(" -> ")));
        }

        let probed = git::probe_branch(url, branch);
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
        git::ensure_checkout(&db, &sha, &dest);

        let dep_manifest = Manifest::load_from(&dest).unwrap_or_else(|err| {
            cli::exit_with_error(format!("Dependency `{key}` manifest error:\n{err}"));
        });

        self.stack.push(key.to_owned());

        let mut refs = Vec::with_capacity(dep_manifest.dependencies.len());

        for (child_name, child_dep) in &dep_manifest.dependencies {
            let (child_url, child_branch) = dependency_parts(child_dep);
            let child_sha = self.visit(child_name, child_url, child_branch);

            refs.push(LockDependency {
                package: child_name.clone(),
                rev: child_sha,
            });
        }

        self.stack.pop();

        let node = LockPackage {
            name: key.to_owned(),
            version: dep_manifest.package.version,
            zeen: dep_manifest.package.zeen,
            dependencies: refs,
            repository: Some(url.clone()),
            branch: Some(probed),
            commit: Some(sha.clone()),
        };

        self.index.insert(key.to_owned(), self.nodes.len());
        self.nodes.push(node);

        sha
    }
}

fn dependency_parts(dependency: &Dependency) -> (&url::Url, Option<String>) {
    match dependency {
        Dependency::Short(url) => (url, None),
        Dependency::Detailed { git, branch } => (git, branch.clone()),
    }
}
