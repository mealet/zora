use std::{collections::HashSet, fs, path::Path};

use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

use crate::manifest::{Build, LoadError, Manifest};

pub const LOCKFILE_CONFIG: &str = "Zora.lock";
pub const SUPPORTED_LOCK_VERSION: u32 = 3;

#[derive(Debug, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    pub package: Vec<LockPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockPackage {
    pub name: String,
    pub version: Version,
    pub zeen: VersionReq,
    #[serde(
        deserialize_with = "deserialize_dependencies",
        serialize_with = "serialize_dependencies"
    )]
    pub dependencies: Vec<LockDependency>,

    pub repository: Option<Url>,
    pub branch: Option<String>,
    pub commit: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<Build>,
}

/// Dependency structure from format "name@rev"
#[derive(Debug, Clone, Serialize, Hash, Eq, PartialEq)]
pub struct LockDependency {
    pub package: String,
    pub rev: String,
}

fn deserialize_dependencies<'de, D>(deserializer: D) -> Result<Vec<LockDependency>, D::Error>
where
    D: Deserializer<'de>,
{
    let dependencies = Vec::<String>::deserialize(deserializer)?;

    dependencies
        .into_iter()
        .map(|value| {
            let (package, rev) = value.split_once('@').ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "invalid dependency `{value}`, expected `package@rev`"
                ))
            })?;

            Ok(LockDependency {
                package: package.to_owned(),
                rev: rev.to_owned(),
            })
        })
        .collect()
}

fn serialize_dependencies<S>(
    dependencies: &[LockDependency],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let values = dependencies
        .iter()
        .map(|dependency| format!("{}@{}", dependency.package, dependency.rev))
        .collect::<Vec<_>>();

    values.serialize(serializer)
}

impl Lockfile {
    pub fn load(root: &Path) -> Result<Self, LoadError> {
        let lockfile_path = root.join(LOCKFILE_CONFIG);
        if !lockfile_path.is_file() {
            return Err(LoadError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Project's lockfile not found in root",
            )));
        }

        let lockfile_content = fs::read_to_string(&lockfile_path)?;
        let lockfile: Self = toml::from_str(&lockfile_content)?;

        Ok(lockfile)
    }

    pub fn validate(&self, zeen_installed: &Version) -> Result<(), LockValidateError> {
        if self.version != SUPPORTED_LOCK_VERSION {
            return Err(LockValidateError::UnsupportedVersion(self.version));
        }

        // this probably shouldn't happen, but I'll keep this error
        let Some(root_graph) = self.package.first() else {
            return Err(LockValidateError::EmptyGraph);
        };

        if root_graph.repository.is_some()
            || root_graph.branch.is_some()
            || root_graph.commit.is_some()
        {
            return Err(LockValidateError::RootHasSource);
        }

        let mut cached_deps: HashSet<LockDependency> = HashSet::new();

        for (idx, package) in self.package.iter().enumerate() {
            let version_requirement = &package.zeen;

            if !version_requirement.matches(zeen_installed) {
                return Err(LockValidateError::ZeenMismatch {
                    package: package.name.clone(),
                    required: package.zeen.clone(),
                    installed: zeen_installed.clone(),
                });
            }

            if idx != 0
                && (package
                    .repository
                    .as_ref()
                    .map(|x| x.to_string())
                    .unwrap_or_default()
                    .is_empty()
                    || package.commit.clone().unwrap_or_default().is_empty())
            {
                return Err(LockValidateError::IncompleteNode(package.name.clone()));
            }

            let dependency_format = LockDependency {
                package: package.name.clone(),
                rev: package.commit.clone().unwrap_or("HEAD".into()),
            };

            if !cached_deps.insert(dependency_format) {
                return Err(LockValidateError::DuplicateNode {
                    package: package.name.clone(),
                    rev: package.commit.clone().unwrap_or("HEAD".into()),
                });
            }
        }

        // dependencies verifier
        for package in self.package.iter() {
            for dep in package.dependencies.iter() {
                if !cached_deps.contains(dep) {
                    return Err(LockValidateError::MissingNode {
                        wanted_by: package.name.clone(),
                        package: dep.package.clone(),
                        rev: dep.rev.clone(),
                    });
                }
            }
        }

        Ok(())
    }
}

pub enum LockValidateError {
    UnsupportedVersion(u32),
    EmptyGraph,
    RootHasSource,
    ZeenMismatch {
        package: String,
        required: VersionReq,
        installed: Version,
    },
    MissingNode {
        wanted_by: String,
        package: String,
        rev: String,
    },
    DuplicateNode {
        package: String,
        rev: String,
    },
    IncompleteNode(String),
}

impl LockPackage {
    pub fn from_manifest(manifest: &Manifest) -> Self {
        Self {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            zeen: manifest.package.zeen.clone(),
            dependencies: vec![],
            repository: None,
            branch: None,
            commit: None,
            build: manifest.build.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str) -> Manifest {
        let text = format!(
            r#"
[package]
name = "{name}"
version = "0.1.0"
zeen = "^0.1.0"
type = "bin"
"#
        );

        toml::from_str(&text).unwrap()
    }

    #[test]
    fn lockfile_roundtrips_dependencies() {
        let mut root = LockPackage::from_manifest(&manifest("root"));
        root.dependencies = vec![LockDependency {
            package: "dep".to_owned(),
            rev: "abc123".to_owned(),
        }];

        let lockfile = Lockfile {
            version: SUPPORTED_LOCK_VERSION,
            package: vec![root],
        };

        let text = toml::to_string_pretty(&lockfile).unwrap();
        let parsed: Lockfile = toml::from_str(&text).unwrap();

        assert_eq!(parsed.package[0].dependencies.len(), 1);
        assert_eq!(parsed.package[0].dependencies[0].package, "dep");
        assert_eq!(parsed.package[0].dependencies[0].rev, "abc123");
    }

    #[test]
    fn dependency_requires_revision_separator() {
        let text = r#"
version = 1

[[package]]
name = "root"
version = "0.1.0"
zeen = "^0.1.0"
dependencies = ["broken"]
"#;

        let parsed: Result<Lockfile, _> = toml::from_str(text);

        assert!(parsed.is_err());
    }

    #[test]
    fn from_manifest_captures_link_metadata() {
        let text = r#"
[package]
name = "zaylib"
version = "0.1.0"
zeen = "^0.3.0"
type = "lib/static"

[build]
output = "zaylib"
link = ["raylib"]
link-dir = ["vendor/lib"]
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();
        let package = LockPackage::from_manifest(&manifest);
        let build = package.build.unwrap();

        assert_eq!(build.link.as_deref().unwrap(), ["raylib"]);
        assert_eq!(build.link_dir.as_deref().unwrap(), ["vendor/lib"]);
    }

    #[test]
    fn lockfile_roundtrips_platform_build_spec() {
        let text = r#"
[package]
name = "root"
version = "0.1.0"
zeen = "^0.1.0"
type = "bin"

[build]
output = "root"
link = ["raylib"]

[build.os.linux]
link = ["m", "pthread"]

[build.target.x86_64-unknown-linux-gnu]
link = ["rt"]
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();

        let lockfile = Lockfile {
            version: SUPPORTED_LOCK_VERSION,
            package: vec![LockPackage::from_manifest(&manifest)],
        };

        let rendered = toml::to_string_pretty(&lockfile).unwrap();
        let parsed: Lockfile = toml::from_str(&rendered).unwrap();
        let build = parsed.package[0].build.as_ref().unwrap();

        assert_eq!(build.link.as_deref().unwrap(), ["raylib"]);
        assert_eq!(build.os["linux"].link.as_deref().unwrap(), ["m", "pthread"]);
        assert_eq!(
            build.target["x86_64-unknown-linux-gnu"]
                .link
                .as_deref()
                .unwrap(),
            ["rt"]
        );
    }

    #[test]
    fn lockfile_omits_empty_build_metadata() {
        let root = LockPackage::from_manifest(&manifest("root"));

        let lockfile = Lockfile {
            version: SUPPORTED_LOCK_VERSION,
            package: vec![root],
        };

        let text = toml::to_string_pretty(&lockfile).unwrap();

        assert!(!text.contains("build"));
        assert!(!text.contains("link"));
    }
}
