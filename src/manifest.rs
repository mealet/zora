use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Display,
    fs, io,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub package: Package,
    pub build: Option<Build>,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: Version,
    pub zeen: VersionReq,
    pub r#type: PackageType,

    pub authors: Option<Vec<String>>,
    pub repository: Option<Url>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum PackageType {
    #[serde(rename = "bin")]
    Bin,

    #[serde(rename = "lib/static")]
    Static,

    #[serde(rename = "lib/dynamic")]
    Dynamic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub output: String,
    pub flags: Option<Vec<String>>,
    pub link: Option<Vec<String>>,
    #[serde(rename = "link-dir")]
    pub link_dir: Option<Vec<String>>,
}

impl Build {
    pub fn link_libs(&self) -> &[String] {
        self.link.as_deref().unwrap_or(&[])
    }

    pub fn link_dirs(&self) -> &[String] {
        self.link_dir.as_deref().unwrap_or(&[])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    Short(Url),
    Detailed { git: Url, rev: Option<String> },
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
        let manifest_deserialized: Self = toml::from_str(&manifest_content)?;

        let root_path = manifest_config
            .parent()
            .ok_or(io::Error::new(
                io::ErrorKind::NotFound,
                "unable to detect project's root (manifest found, but directory not)",
            ))?
            .to_path_buf();

        Ok((manifest_deserialized, root_path))
    }

    pub fn load_explicit(path: &Path) -> Result<(Self, PathBuf), LoadError> {
        let manifest_content = fs::read_to_string(path)?;
        let manifest_deserialized: Self = toml::from_str(&manifest_content)?;

        let root_path = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        Ok((manifest_deserialized, root_path))
    }

    pub fn load_from(dir: &Path) -> Result<Self, LoadError> {
        let manifest_path = dir.join(MANIFEST_CONFIG);
        let manifest_content = fs::read_to_string(manifest_path)?;
        let manifest_deserialized: Self = toml::from_str(&manifest_content)?;

        Ok(manifest_deserialized)
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

impl Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Io(err) => err.to_string(),
                Self::Toml(err) => err.to_string(),
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_parses_link_fields() {
        let text = r#"
[package]
name = "zaylib"
version = "0.1.0"
zeen = "^0.3.0"
type = "lib/static"

[build]
output = "zaylib"
link = ["raylib", "m"]
link-dir = ["vendor/lib"]
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();
        let build = manifest.build.unwrap();

        assert_eq!(build.link_libs(), ["raylib", "m"]);
        assert_eq!(build.link_dirs(), ["vendor/lib"]);
    }

    #[test]
    fn build_without_link_fields_defaults_empty() {
        let text = r#"
[package]
name = "app"
version = "0.1.0"
zeen = "^0.3.0"
type = "bin"

[build]
output = "app"
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();
        let build = manifest.build.unwrap();

        assert!(build.link_libs().is_empty());
        assert!(build.link_dirs().is_empty());
    }
}
