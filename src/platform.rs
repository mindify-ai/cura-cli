use std::{fs, io::Write, process::Command};

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Apt,
    Dnf,
    Zypper,
}

#[derive(Debug, Clone, Serialize)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub distro: Option<String>,
    pub distro_version: Option<String>,
    pub wsl: bool,
    pub package_manager: Option<PackageManager>,
}

impl Platform {
    pub fn detect() -> Self {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        if os != "linux" {
            return Self {
                os,
                arch,
                distro: None,
                distro_version: None,
                wsl: false,
                package_manager: None,
            };
        }
        let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
        let field = |key: &str| {
            release.lines().find_map(|line| {
                line.strip_prefix(&format!("{key}="))
                    .map(|v| v.trim_matches('"').to_string())
            })
        };
        let distro = field("ID");
        let like = field("ID_LIKE").unwrap_or_default();
        let family = format!("{} {like}", distro.clone().unwrap_or_default());
        let package_manager = if family.contains("debian") || family.contains("ubuntu") {
            Some(PackageManager::Apt)
        } else if family.contains("rhel") || family.contains("fedora") || family.contains("centos")
        {
            Some(PackageManager::Dnf)
        } else if family.contains("suse") {
            Some(PackageManager::Zypper)
        } else {
            None
        };
        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .to_lowercase();
        let wsl = kernel.contains("microsoft") || std::env::var_os("WSL_INTEROP").is_some();
        Self {
            os,
            arch,
            distro,
            distro_version: field("VERSION_ID"),
            wsl,
            package_manager,
        }
    }

    pub fn ensure_cuda_supported(&self) -> Result<()> {
        if self.os != "linux" {
            bail!(
                "CUDA environments are supported on Linux and WSL; this host is {}. Use CURA on the target Linux machine",
                self.os
            );
        }
        if !matches!(self.arch.as_str(), "x86_64" | "aarch64") {
            bail!(
                "unsupported architecture {}; CURA supports x86_64 and aarch64",
                self.arch
            );
        }
        Ok(())
    }

    pub fn manifest_key(&self) -> Result<&'static str> {
        match self.arch.as_str() {
            "x86_64" => Ok("linux-x86_64"),
            "aarch64" => Ok("linux-sbsa"),
            _ => bail!("unsupported architecture {}", self.arch),
        }
    }

    pub fn display_name(&self) -> String {
        if self.wsl {
            format!("WSL {}", self.arch)
        } else if let Some(distro) = &self.distro {
            format!("{} {}", distro, self.arch)
        } else {
            format!("{} {}", self.os, self.arch)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DriverStatus {
    pub gpu_present: bool,
    pub version: Option<String>,
    pub wsl_managed: bool,
}

pub fn driver_status(platform: &Platform) -> DriverStatus {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=driver_version", "--format=csv,noheader"])
        .output();
    let version = output.ok().filter(|o| o.status.success()).and_then(|o| {
        String::from_utf8(o.stdout)
            .ok()?
            .lines()
            .next()
            .map(str::trim)
            .map(str::to_string)
    });
    let gpu_present = version.is_some()
        || Command::new("lspci").output().ok().is_some_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("nvidia")
        });
    DriverStatus {
        gpu_present,
        version,
        wsl_managed: platform.wsl,
    }
}

pub fn driver_compatible(driver: &str, cuda_major: u64) -> bool {
    let required = match cuda_major {
        13.. => (580, 65),
        12 => (525, 60),
        11 => (450, 80),
        _ => return true,
    };
    let mut parts = driver.split('.').filter_map(|p| p.parse::<u64>().ok());
    let actual = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    actual >= required
}

pub fn run_privileged(program: &str, args: &[String]) -> Result<()> {
    let status = Command::new("sudo")
        .arg(program)
        .args(args)
        .status()
        .with_context(|| format!("run sudo {program}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

/// Configure NVIDIA's official CUDA repository for the detected distribution.
/// This is only called after the user approves a privileged install plan.
pub fn ensure_cuda_repository(platform: &Platform) -> Result<()> {
    let manager = platform
        .package_manager
        .context("official NVIDIA repositories are supported through apt, dnf, or zypper")?;
    let distro = platform.distro.as_deref().unwrap_or_default();
    let version = platform.distro_version.as_deref().unwrap_or_default();
    let major = version.split('.').next().unwrap_or(version);
    let compact = version.replace('.', "");
    let slug = match manager {
        PackageManager::Apt if distro == "ubuntu" => format!("ubuntu{compact}"),
        PackageManager::Apt if distro == "debian" => format!("debian{major}"),
        PackageManager::Dnf if distro == "fedora" => format!("fedora{major}"),
        PackageManager::Dnf => format!("rhel{major}"),
        PackageManager::Zypper if distro.contains("opensuse") => format!("opensuse{major}"),
        PackageManager::Zypper => format!("sles{major}"),
        _ => bail!("cannot map {distro} {version} to an NVIDIA CUDA repository"),
    };
    let arch = if platform.arch == "x86_64" {
        "x86_64"
    } else {
        "sbsa"
    };
    let base = format!("https://developer.download.nvidia.com/compute/cuda/repos/{slug}/{arch}");
    match manager {
        PackageManager::Apt => {
            let url = format!("{base}/cuda-keyring_1.1-1_all.deb");
            let bytes = reqwest::blocking::get(&url)?
                .error_for_status()
                .with_context(|| format!("NVIDIA has no supported repository at {base}"))?
                .bytes()?;
            let mut file = tempfile::Builder::new().suffix(".deb").tempfile()?;
            file.write_all(&bytes)?;
            run_privileged("dpkg", &["-i".into(), file.path().display().to_string()])?;
            run_privileged("apt-get", &["update".into()])?;
        }
        PackageManager::Dnf => {
            install_repo_file(&format!("{base}/cuda-{slug}.repo"), RepoKind::Dnf)?;
            run_privileged("dnf", &["makecache".into()])?;
        }
        PackageManager::Zypper => {
            install_repo_file(&format!("{base}/cuda-{slug}.repo"), RepoKind::Zypper)?;
            run_privileged(
                "zypper",
                &["--gpg-auto-import-keys".into(), "refresh".into()],
            )?;
        }
    }
    Ok(())
}

enum RepoKind {
    Dnf,
    Zypper,
}

fn install_repo_file(url: &str, kind: RepoKind) -> Result<()> {
    let bytes = reqwest::blocking::get(url)?
        .error_for_status()
        .with_context(|| format!("NVIDIA has no supported repository at {url}"))?
        .bytes()?;
    let mut file = tempfile::Builder::new().suffix(".repo").tempfile()?;
    file.write_all(&bytes)?;
    let destination = match kind {
        RepoKind::Dnf => "/etc/yum.repos.d/cuda-nvidia.repo",
        RepoKind::Zypper => "/etc/zypp/repos.d/cuda-nvidia.repo",
    };
    run_privileged(
        "install",
        &[
            "-m".into(),
            "0644".into(),
            file.path().display().to_string(),
            destination.into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_driver_floors() {
        assert!(driver_compatible("535.104.05", 12));
        assert!(!driver_compatible("520.10", 12));
        assert!(!driver_compatible("575.99", 13));
    }
}
