//! Access to `cargo metadata`.

use crate::process::Cmd;
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The kind of a dependency declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

impl Kind {
    /// The kind's name in messages: `normal`, `dev` or `build`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Dev => "dev",
            Self::Build => "build",
        }
    }
}

/// One dependency declaration of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Name of the package depended on.
    pub name: String,
    /// Declaration kind.
    pub kind: Kind,
    /// Whether the dependency is optional.
    pub optional: bool,
    /// Whether the dependency's default features are enabled.
    pub default_features: bool,
    /// Features of the dependency the declaration enables.
    pub features: Vec<String>,
}

/// A binary target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binary {
    /// Target name.
    pub name: String,
    /// Path of the target's root source file.
    pub src_path: PathBuf,
}

/// A workspace member as reported by `cargo metadata`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Package {
    /// Package name.
    pub name: String,
    /// Path of the package's `Cargo.toml`.
    pub manifest_path: String,
    /// Names of the package's normal (non-dev, non-build) dependencies, in declaration order:
    /// the names of the [`Kind::Normal`] entries of `dependencies`.
    pub normal_dependencies: Vec<String>,
    /// Every dependency declaration, of every kind.
    pub dependencies: Vec<Dependency>,
    /// The `[features]` table, including implicit features of optional dependencies.
    pub features: BTreeMap<String, Vec<String>>,
    /// Binary targets.
    pub binaries: Vec<Binary>,
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

/// Parses the package list of `cargo metadata --no-deps --format-version 1` output.
///
/// # Errors
/// Fails if the JSON does not have the `cargo metadata` shape or names an unknown dependency
/// kind.
pub fn parse_members(json: &str) -> Result<Vec<Package>> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))?;
    value["packages"]
        .as_array()
        .ok_or_else(|| Error::Parse("`packages` is not an array".into()))?
        .iter()
        .map(parse_package)
        .collect()
}

fn string(value: &serde_json::Value, what: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Parse(format!("cargo metadata without a string `{what}`")))
}

/// The strings of `value`, an array or absent.
fn strings(value: &serde_json::Value, what: &str) -> Result<Vec<String>> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| string(s, what))
        .collect()
}

fn parse_dependency(package: &str, d: &serde_json::Value) -> Result<Dependency> {
    let kind = match d["kind"].as_str() {
        None => Kind::Normal,
        Some("dev") => Kind::Dev,
        Some("build") => Kind::Build,
        Some(other) => {
            return Err(Error::Parse(format!(
                "{package}: unknown dependency kind `{other}`"
            )));
        }
    };
    Ok(Dependency {
        name: string(&d["name"], "dependencies[].name")?,
        kind,
        optional: d["optional"].as_bool().unwrap_or(false),
        default_features: d["uses_default_features"].as_bool().unwrap_or(true),
        features: strings(&d["features"], "dependencies[].features[]")?,
    })
}

fn parse_package(p: &serde_json::Value) -> Result<Package> {
    let name = string(&p["name"], "name")?;
    let dependencies: Vec<Dependency> = p["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| parse_dependency(&name, d))
        .collect::<Result<_>>()?;
    let features = p["features"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(feature, enables)| Ok((feature.clone(), strings(enables, "features[]")?)))
        .collect::<Result<_>>()?;
    let binaries = p["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "bin"))
        })
        .map(|t| {
            Ok(Binary {
                name: string(&t["name"], "targets[].name")?,
                src_path: PathBuf::from(string(&t["src_path"], "targets[].src_path")?),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Package {
        manifest_path: string(&p["manifest_path"], "manifest_path")?,
        normal_dependencies: dependencies
            .iter()
            .filter(|d| d.kind == Kind::Normal)
            .map(|d| d.name.clone())
            .collect(),
        name,
        dependencies,
        features,
        binaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(json: &str) -> Package {
        let mut members = parse_members(json).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(members.len(), 1, "{members:?}");
        members.remove(0)
    }

    #[test]
    fn reads_kinds_features_and_binaries() {
        let json = r#"{"packages":[{
            "name":"op",
            "manifest_path":"/w/op/Cargo.toml",
            "dependencies":[
                {"name":"fakes","kind":null,"optional":true,"uses_default_features":true,"features":[]},
                {"name":"devtools","kind":"dev","optional":false,"uses_default_features":false},
                {"name":"cc","kind":"build","optional":false,"uses_default_features":true,"features":["parallel"]},
                {"name":"serde"}
            ],
            "features":{"m0-fakes":["dep:fakes"],"default":[]},
            "targets":[
                {"kind":["bin"],"name":"op","src_path":"/w/op/src/main.rs"},
                {"kind":["lib"],"name":"op","src_path":"/w/op/src/lib.rs"},
                {"kind":["test"],"name":"t","src_path":"/w/op/tests/t.rs"}
            ]
        }]}"#;
        let dep = |name: &str, kind| Dependency {
            name: name.to_owned(),
            kind,
            optional: false,
            default_features: true,
            features: Vec::new(),
        };
        let expected = Package {
            name: "op".to_owned(),
            manifest_path: "/w/op/Cargo.toml".to_owned(),
            normal_dependencies: vec!["fakes".to_owned(), "serde".to_owned()],
            dependencies: vec![
                Dependency {
                    optional: true,
                    ..dep("fakes", Kind::Normal)
                },
                Dependency {
                    default_features: false,
                    ..dep("devtools", Kind::Dev)
                },
                Dependency {
                    features: vec!["parallel".to_owned()],
                    ..dep("cc", Kind::Build)
                },
                dep("serde", Kind::Normal),
            ],
            features: BTreeMap::from([
                ("default".to_owned(), Vec::new()),
                ("m0-fakes".to_owned(), vec!["dep:fakes".to_owned()]),
            ]),
            binaries: vec![Binary {
                name: "op".to_owned(),
                src_path: PathBuf::from("/w/op/src/main.rs"),
            }],
        };
        assert_eq!(parse_one(json), expected);
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let json = r#"{"packages":[{"name":"a","manifest_path":"/w/a/Cargo.toml",
            "dependencies":[{"name":"b","kind":"odd"}]}]}"#;
        let err = parse_members(json).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("unknown dependency kind `odd`"), "{err}");
    }

    #[test]
    fn rejects_a_non_string_feature_entry() {
        let json = r#"{"packages":[{"name":"a","manifest_path":"/w/a/Cargo.toml",
            "features":{"f":[1]}}]}"#;
        assert!(parse_members(json).is_err());
    }

    #[test]
    fn rejects_a_package_without_a_manifest_path() {
        assert!(parse_members(r#"{"packages":[{"name":"a"}]}"#).is_err());
    }

    #[test]
    fn kind_names() {
        assert_eq!(
            [Kind::Normal, Kind::Dev, Kind::Build].map(Kind::name),
            ["normal", "dev", "build"]
        );
    }
}
