use std::{fmt, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Runtime,
    Toolkit,
    Full,
}

impl Profile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Runtime => "Runtime",
            Self::Toolkit => "Toolkit",
            Self::Full => "Full",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Runtime => "runtime and core math libraries",
            Self::Toolkit => "runtime, nvcc, headers, and developer tools",
            Self::Full => "all supported non-driver toolkit components",
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label().to_lowercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSpec {
    pub major: u64,
    pub minor: Option<u64>,
    pub patch: Option<u64>,
}

impl VersionSpec {
    pub fn matches(&self, version: &Version) -> bool {
        version.major == self.major
            && self.minor.is_none_or(|v| version.minor == v)
            && self.patch.is_none_or(|v| version.patch == v)
    }
}

impl FromStr for VersionSpec {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self> {
        let value = raw.trim().strip_prefix("cuda-").unwrap_or(raw.trim());
        let parts: Vec<_> = value.split('.').collect();
        if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
            bail!("invalid CUDA version {raw:?}; use cuda-12, cuda-12.9, or cuda-12.9.1");
        }
        let parse = |index: usize| -> Result<Option<u64>> {
            parts
                .get(index)
                .map(|p| {
                    p.parse()
                        .with_context(|| format!("invalid CUDA version {raw:?}"))
                })
                .transpose()
        };
        Ok(Self {
            major: parse(0)?.context("CUDA major version is required")?,
            minor: parse(1)?,
            patch: parse(2)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub version: String,
    pub relative_path: String,
    pub sha256: String,
    pub size: u64,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentManifest {
    pub release: Version,
    pub profile: Profile,
    pub scope: InstallScope,
    pub platform: String,
    pub installed_at_unix: u64,
    pub prefix: PathBuf,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallScope {
    User,
    System,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonEnvelope<T: Serialize> {
    pub status: &'static str,
    pub data: T,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases() {
        let spec: VersionSpec = "cuda-12.9".parse().unwrap();
        assert_eq!(spec.major, 12);
        assert_eq!(spec.minor, Some(9));
        assert_eq!(spec.patch, None);
        assert!(spec.matches(&Version::new(12, 9, 2)));
        assert!(!spec.matches(&Version::new(12, 8, 2)));
    }

    #[test]
    fn rejects_bad_versions() {
        assert!("cuda-12.9.1.2".parse::<VersionSpec>().is_err());
        assert!("cuda-twelve".parse::<VersionSpec>().is_err());
    }
}
