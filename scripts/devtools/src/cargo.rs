//! Access to `cargo metadata`.

use crate::process::Cmd;
use crate::{Error, Result};
use std::path::Path;

/// A workspace member as reported by `cargo metadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Package name.
    pub name: String,
    /// Path of the package's `Cargo.toml`.
    pub manifest_path: String,
    /// Names of the package's normal (non-dev, non-build) dependencies.
    pub normal_dependencies: Vec<String>,
}

/// Returns the workspace members of the workspace containing `dir`.
///
/// # Errors
/// Fails if `cargo metadata` fails or prints unexpected JSON.
pub fn workspace_members(dir: impl AsRef<Path>) -> Result<Vec<Package>> {
    let json = Cmd::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(dir)
        .output()?;
    parse_members(&json)
}

/// Parses the package list of `cargo metadata --no-deps` output.
///
/// # Errors
/// Fails if the JSON does not have the `cargo metadata` shape.
pub fn parse_members(json: &str) -> Result<Vec<Package>> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))?;
    let packages = value["packages"]
        .as_array()
        .ok_or_else(|| Error::Parse("`packages` is not an array".into()))?;
    packages
        .iter()
        .map(|p| {
            let field = |key: &str| {
                p[key]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::Parse(format!("package without `{key}`")))
            };
            let normal_dependencies = p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["kind"].is_null())
                .filter_map(|d| d["name"].as_str().map(str::to_owned))
                .collect();
            Ok(Package {
                name: field("name")?,
                manifest_path: field("manifest_path")?,
                normal_dependencies,
            })
        })
        .collect()
}
