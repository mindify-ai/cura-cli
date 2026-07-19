use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use regex::Regex;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;

use crate::{
    model::{Component, Profile, VersionSpec},
    paths::CuraPaths,
};

const REDIST_ROOT: &str = "https://developer.download.nvidia.com/compute/cuda/redist";

pub struct Catalog {
    client: Client,
    cache: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RawPlatform {
    relative_path: String,
    sha256: String,
    #[serde(default)]
    size: String,
}

#[derive(Debug, Deserialize)]
struct RawComponent {
    #[serde(default)]
    license: String,
    #[serde(default)]
    version: String,
    #[serde(flatten)]
    platforms: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    release_label: String,
    #[serde(flatten)]
    components: BTreeMap<String, serde_json::Value>,
}

impl Catalog {
    pub fn new(paths: &CuraPaths) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("cura/", env!("CARGO_PKG_VERSION")))
                .build()?,
            cache: paths.cache.join("catalog"),
        })
    }

    pub fn versions(&self) -> Result<Vec<Version>> {
        fs::create_dir_all(&self.cache)?;
        let cache_file = self.cache.join("index.html");
        let body = match self
            .client
            .get(format!("{REDIST_ROOT}/"))
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
        {
            Ok(body) => {
                fs::write(&cache_file, &body)?;
                body
            }
            Err(error) => fs::read_to_string(&cache_file).with_context(|| {
                format!("fetch NVIDIA release catalog ({error}); no cached catalog is available")
            })?,
        };
        let re = Regex::new(r#"redistrib_([0-9]+\.[0-9]+(?:\.[0-9]+)?)\.json"#)?;
        let mut versions: Vec<_> = re
            .captures_iter(&body)
            .filter_map(|c| Version::parse(&c[1]).ok())
            .collect();
        versions.sort();
        versions.dedup();
        Ok(versions)
    }

    pub fn resolve(&self, spec: &VersionSpec) -> Result<Version> {
        self.versions()?
            .into_iter()
            .filter(|v| spec.matches(v))
            .max()
            .with_context(|| {
                format!(
                    "no published CUDA release matches cuda-{}{}{}",
                    spec.major,
                    spec.minor.map(|v| format!(".{v}")).unwrap_or_default(),
                    spec.patch.map(|v| format!(".{v}")).unwrap_or_default()
                )
            })
    }

    pub fn components(
        &self,
        release: &Version,
        platform_key: &str,
        profile: Profile,
    ) -> Result<Vec<Component>> {
        fs::create_dir_all(&self.cache)?;
        let cache_file = self.cache.join(format!("redistrib_{release}.json"));
        let url = format!("{REDIST_ROOT}/redistrib_{release}.json");
        let bytes = match self
            .client
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
        {
            Ok(bytes) => {
                fs::write(&cache_file, &bytes)?;
                bytes.to_vec()
            }
            Err(error) => fs::read(&cache_file).with_context(|| {
                format!("fetch {url} ({error}); no cached manifest is available")
            })?,
        };
        let raw: RawManifest =
            serde_json::from_slice(&bytes).context("parse NVIDIA redistributable manifest")?;
        if raw.release_label != release.to_string() {
            bail!(
                "manifest release mismatch: expected {release}, got {}",
                raw.release_label
            );
        }
        let mut result = Vec::new();
        for (name, value) in raw.components {
            if matches!(
                name.as_str(),
                "release_date" | "release_product" | "release_label"
            ) || !included(&name, profile)
            {
                continue;
            }
            let Ok(raw_component) = serde_json::from_value::<RawComponent>(value) else {
                continue;
            };
            let Some(value) = raw_component.platforms.get(platform_key) else {
                continue;
            };
            let Ok(p) = serde_json::from_value::<RawPlatform>(value.clone()) else {
                continue;
            };
            result.push(Component {
                name,
                version: raw_component.version,
                relative_path: p.relative_path,
                sha256: p.sha256,
                size: p.size.parse().unwrap_or(0),
                license: raw_component.license,
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        if result.is_empty() {
            bail!("NVIDIA manifest has no {profile} components for {platform_key}");
        }
        Ok(result)
    }

    pub fn component_url(component: &Component) -> String {
        format!("{REDIST_ROOT}/{}", component.relative_path)
    }
}

fn included(name: &str, profile: Profile) -> bool {
    const RUNTIME: &[&str] = &[
        "cuda_cudart",
        "cuda_nvrtc",
        "libcublas",
        "libcufft",
        "libcurand",
        "libcusolver",
        "libcusparse",
        "libnpp",
        "libnvjitlink",
        "libnvjpeg",
        "cuda_opencl",
    ];
    const TOOLKIT: &[&str] = &[
        "cuda_cccl",
        "cuda_crt",
        "cuda_cuobjdump",
        "cuda_cuxxfilt",
        "cuda_nvcc",
        "cuda_nvdisasm",
        "cuda_nvprune",
        "cuda_culibos",
        "cuda_cupti",
        "cuda_nvtx",
        "cuda_profiler_api",
        "nvvm",
    ];
    let forbidden = name.contains("compat")
        || name.contains("driver")
        || name.contains("cross")
        || name.contains("orin");
    if forbidden {
        return false;
    }
    RUNTIME.contains(&name)
        || matches!(profile, Profile::Toolkit | Profile::Full) && TOOLKIT.contains(&name)
        || matches!(profile, Profile::Full)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_are_monotonic_and_drivers_excluded() {
        assert!(included("cuda_cudart", Profile::Runtime));
        assert!(!included("cuda_nvcc", Profile::Runtime));
        assert!(included("cuda_nvcc", Profile::Toolkit));
        assert!(included("cuda_gdb", Profile::Full));
        assert!(!included("cuda_compat", Profile::Full));
    }
}
