use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::PathBuf};

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
    pub version: String,
    pub zeen: String,
    pub r#type: PackageType,

    pub authors: Option<Vec<String>>,
    pub repository: Option<String>,
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
    Detailed { git: String, branch: Option<String> },
}

// Load implementation

/// Maximum depth of parent folders where to search for manifest
pub const PARENT_CONFIG_DEPTH: usize = 3;
/// Name of manifest config file
pub const MANIFEST_CONFIG: &str = "Zora.toml";

impl Manifest {
    pub fn load() -> Result<(Self, PathBuf), LoadError> {
        let cwd = std::env::current_dir()?;

        let mut cursor = cwd;
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
            "manifest `Zora.toml` not found",
        ))?;

        let manifest_content = fs::read_to_string(&manifest_config)?;
        let manifest: Self = toml::from_str(&manifest_content)?;

        Ok((manifest, manifest_config))
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
