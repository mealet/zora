use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::PathBuf};
use url::Url;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub package: Package,
    pub build: Option<Build>,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: Version,
    pub zeen: VersionReq,
    pub r#type: PackageType,

    pub authors: Option<Vec<String>>,
    pub repository: Option<Url>,
    pub license: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PackageType {
    #[serde(rename = "bin")]
    Bin,

    #[serde(rename = "lib/static")]
    Static,

    #[serde(rename = "lib/dynamic")]
    Dynamic,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Build {
    pub output: String,
    pub flags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    Short(String),
    Detailed { git: Url, branch: Option<String> },
}

// Load implementation

/// Maximum depth of parent folders where to search for manifest
pub const PARENT_CONFIG_DEPTH: usize = 3;
/// Name of manifest config file
pub const MANIFEST_CONFIG: &str = "Zora.toml";

impl Manifest {
    pub fn load() -> Result<(Self, PathBuf), LoadError> {
        let cwd = std::env::current_dir()?;

        let mut cursor = cwd.clone();
        let mut depth = 0;
        let mut manifest_path: Option<PathBuf> = None;

        while let None = manifest_path
            && depth < PARENT_CONFIG_DEPTH
            && cursor.exists()
        {
            let file = cursor.join(MANIFEST_CONFIG);

            if file.exists() {
                manifest_path = Some(file);
                break;
            }

            let Some(parent) = cursor.parent() else { break };

            cursor = parent.to_path_buf();
            depth += 1;
        }

        let manifest_config = manifest_path.ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "manifest `Zora.toml` not found (in `{}`)",
                cwd.to_string_lossy()
            ),
        ))?;

        let manifest_content = fs::read_to_string(&manifest_config)?;
        let manifest: Self = toml::from_str(&manifest_content)?;

        let root_path = manifest_config
            .parent()
            .ok_or(io::Error::new(
                io::ErrorKind::NotFound,
                "unable to detect project's root (manifest found, but directory not)",
            ))?
            .to_path_buf();

        Ok((manifest, root_path))
    }
}

/// Enum of errors on Manifest load
#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    Toml(toml::de::Error),
}

impl From<io::Error> for LoadError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<toml::de::Error> for LoadError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}
