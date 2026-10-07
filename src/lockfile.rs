use std::{fs, path::Path};

use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

use crate::manifest::{LoadError, Manifest};

pub const LOCKFILE_CONFIG: &str = "Zora.lock";
pub const SUPPORTED_LOCK_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    pub package: Vec<LockPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
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
}

#[derive(Debug, Serialize)]
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
    pub fn fresh(manifest: &Manifest) -> Self {
        Self {
            version: SUPPORTED_LOCK_VERSION,
            package: vec![LockPackage::from_manifest(manifest)],
        }
    }

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

    pub fn sync_root(&mut self, manifest: &Manifest) -> bool {
        let Some(root) = self.package.first_mut() else {
            self.package.push(LockPackage::from_manifest(manifest));
            return true;
        };

        let changed = !(root.name == manifest.package.name
            && root.version == manifest.package.version
            && root.zeen == manifest.package.zeen
            && root.repository == manifest.package.repository);

        if changed {
            let synced_package = LockPackage::from_manifest(manifest);
            *root = synced_package;
        }

        changed
    }
}

impl LockPackage {
    pub fn from_manifest(manifest: &Manifest) -> Self {
        Self {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            zeen: manifest.package.zeen.clone(),
            dependencies: vec![],
            repository: manifest.package.repository.clone(),
            branch: None,
            commit: None,
        }
    }
}
