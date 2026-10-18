use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    fs, io,
    path::{Path, PathBuf},
};
use url::Url;

use crate::target::Target;

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
    pub description: Option<String>,
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
    pub output: Option<String>,
    pub flags: Option<Vec<String>>,
    pub link: Option<Vec<String>>,
    #[serde(rename = "link-dir")]
    pub link_dir: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub os: BTreeMap<String, BuildOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arch: BTreeMap<String, BuildOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, BuildOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub family: BTreeMap<String, BuildOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub target: BTreeMap<String, BuildOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOverride {
    pub output: Option<String>,
    pub flags: Option<Vec<String>>,
    pub link: Option<Vec<String>>,
    #[serde(rename = "link-dir")]
    pub link_dir: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedBuild {
    pub output: Option<String>,
    pub flags: Vec<String>,
    pub link: Vec<String>,
    pub link_dir: Vec<String>,
}

impl Build {
    pub fn select(&self, target: &Target) -> ResolvedBuild {
        if let Some(over) = self.target.get(&target.triple) {
            return over.resolved();
        }

        if let Some(over) = self.arch.get(&target.arch) {
            return over.resolved();
        }

        if let Some(over) = self.env.get(&target.env) {
            return over.resolved();
        }

        if let Some(over) = self.os.get(&target.os) {
            return over.resolved();
        }

        if let Some(over) = self.family.get(&target.family) {
            return over.resolved();
        }

        ResolvedBuild {
            output: self.output.clone(),
            flags: self.flags.clone().unwrap_or_default(),
            link: self.link.clone().unwrap_or_default(),
            link_dir: self.link_dir.clone().unwrap_or_default(),
        }
    }

    pub fn overrides(&self) -> impl Iterator<Item = &BuildOverride> {
        self.os
            .values()
            .chain(self.arch.values())
            .chain(self.env.values())
            .chain(self.family.values())
            .chain(self.target.values())
    }
}

impl BuildOverride {
    fn resolved(&self) -> ResolvedBuild {
        ResolvedBuild {
            output: self.output.clone(),
            flags: self.flags.clone().unwrap_or_default(),
            link: self.link.clone().unwrap_or_default(),
            link_dir: self.link_dir.clone().unwrap_or_default(),
        }
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
    use crate::target::Target;

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

        assert_eq!(build.output.as_deref(), Some("zaylib"));
        assert_eq!(build.link.as_deref().unwrap(), ["raylib", "m"]);
        assert_eq!(build.link_dir.as_deref().unwrap(), ["vendor/lib"]);
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

        assert!(build.link.is_none());
        assert!(build.link_dir.is_none());
    }

    #[test]
    fn build_platform_override_replaces_base() {
        let text = r#"
[package]
name = "app"
version = "0.1.0"
zeen = "^0.4.0"
type = "bin"

[build]
output = "app"
link = ["base"]

[build.os.linux]
link = ["m", "pthread"]
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();
        let build = manifest.build.unwrap();

        let linux = build.select(&Target::parse("x86_64-unknown-linux-gnu"));
        assert_eq!(linux.link, ["m", "pthread"]);
        assert_eq!(linux.output.as_deref(), None);

        let windows = build.select(&Target::parse("x86_64-pc-windows-msvc"));
        assert_eq!(windows.link, ["base"]);
        assert_eq!(windows.output.as_deref(), Some("app"));
    }

    #[test]
    fn build_prefers_more_specific_selector() {
        let text = r#"
[package]
name = "app"
version = "0.1.0"
zeen = "^0.4.0"
type = "bin"

[build]
output = "app"

[build.os.linux]
link = ["linux"]

[build.arch.x86_64]
link = ["x86_64"]

[build.target.x86_64-unknown-linux-gnu]
link = ["triple"]
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();
        let build = manifest.build.unwrap();

        let exact = build.select(&Target::parse("x86_64-unknown-linux-gnu"));
        assert_eq!(exact.link, ["triple"]);

        let by_arch = build.select(&Target::parse("x86_64-pc-windows-msvc"));
        assert_eq!(by_arch.link, ["x86_64"]);

        let by_os = build.select(&Target::parse("aarch64-unknown-linux-gnu"));
        assert_eq!(by_os.link, ["linux"]);
    }

    #[test]
    fn build_selects_family_override() {
        let text = r#"
[package]
name = "app"
version = "0.1.0"
zeen = "^0.4.0"
type = "bin"

[build]
output = "app"

[build.family.unix]
link = ["m", "pthread", "dl"]
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();
        let build = manifest.build.unwrap();

        let android = build.select(&Target::parse("aarch64-linux-android"));
        assert_eq!(android.link, ["m", "pthread", "dl"]);

        let windows = build.select(&Target::parse("x86_64-pc-windows-msvc"));
        assert!(windows.link.is_empty());
    }

    #[test]
    fn package_description_parses() {
        let text = r#"
[package]
name = "snake"
version = "0.1.0"
zeen = "^0.3.0"
type = "bin"
description = "a snake game"
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();

        assert_eq!(
            manifest.package.description.as_deref(),
            Some("a snake game")
        );
    }

    #[test]
    fn package_without_description_defaults_empty() {
        let text = r#"
[package]
name = "app"
version = "0.1.0"
zeen = "^0.3.0"
type = "bin"
"#;

        let manifest: Manifest = toml::from_str(text).unwrap();

        assert!(manifest.package.description.is_none());
    }
}
