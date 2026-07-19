use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct CuraPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
}

impl CuraPaths {
    pub fn discover() -> Result<Self> {
        if let Some(home) = env::var_os("CURA_HOME") {
            let root = PathBuf::from(home);
            return Ok(Self {
                config: root.join("config"),
                data: root.join("data"),
                cache: root.join("cache"),
                state: root.join("state"),
            });
        }
        Ok(Self {
            config: dirs::config_dir()
                .context("cannot determine config directory")?
                .join("cura"),
            data: dirs::data_dir()
                .context("cannot determine data directory")?
                .join("cura"),
            cache: dirs::cache_dir()
                .context("cannot determine cache directory")?
                .join("cura"),
            state: dirs::state_dir()
                .unwrap_or(dirs::data_local_dir().context("cannot determine state directory")?)
                .join("cura"),
        })
    }

    pub fn ensure(&self) -> Result<()> {
        for path in [&self.config, &self.data, &self.cache, &self.state] {
            fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
        }
        fs::create_dir_all(self.environments())?;
        fs::create_dir_all(self.downloads())?;
        Ok(())
    }

    pub fn environments(&self) -> PathBuf {
        self.data.join("environments")
    }
    pub fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }
    pub fn global_version_file(&self) -> PathBuf {
        self.config.join("version")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.state.join("install.lock")
    }
    pub fn env_dir(&self, version: &semver::Version) -> PathBuf {
        self.environments().join(format!("cuda-{version}"))
    }
}

pub fn find_project_version(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|p| p.join(".cura-version"))
        .find(|p| p.is_file())
}
