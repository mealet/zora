use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
