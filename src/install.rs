use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component as PathComponent, Path},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use fs2::FileExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::blocking::Client;
use semver::Version;
use sha2::{Digest, Sha256};
use xz2::read::XzDecoder;

use crate::{
    catalog::Catalog,
    cli::InstallArgs,
    model::{Component, EnvironmentManifest, InstallScope, Profile, VersionSpec},
    paths::CuraPaths,
    platform::{self, PackageManager, Platform},
};

#[derive(Debug)]
pub struct InstallPlan {
    pub release: Version,
    pub profile: Profile,
    pub scope: InstallScope,
    pub components: Vec<Component>,
    pub download_bytes: u64,
    pub driver_action: Option<String>,
}

pub fn install(
    args: InstallArgs,
    paths: &CuraPaths,
    interactive: bool,
    quiet: bool,
) -> Result<EnvironmentManifest> {
    let platform = Platform::detect();
    platform.ensure_cuda_supported()?;
    if !interactive && args.profile.is_none() {
        bail!("--profile runtime|toolkit|full is required when input is not interactive");
    }
    let profile = match args.profile {
        Some(profile) => profile,
        None => choose_profile()?,
    };
    let spec: VersionSpec = args.version.parse()?;
    let catalog = Catalog::new(paths)?;
    let release = catalog.resolve(&spec)?;
    let scope = if args.system {
        InstallScope::System
    } else {
        InstallScope::User
    };
    let components = if scope == InstallScope::User {
        catalog.components(&release, platform.manifest_key()?, profile)?
    } else {
        Vec::new()
    };
    let driver = platform::driver_status(&platform);
    let driver_action = if !args.no_driver
        && !platform.wsl
        && driver.gpu_present
        && driver
            .version
            .as_deref()
            .is_none_or(|v| !platform::driver_compatible(v, release.major))
    {
        Some(match driver.version {
            Some(v) => format!("upgrade NVIDIA driver {v} for CUDA {}", release.major),
            None => "install NVIDIA driver".into(),
        })
    } else {
        None
    };
    let plan = InstallPlan {
        release,
        profile,
        scope,
        download_bytes: components.iter().map(|c| c.size).sum(),
        components,
        driver_action,
    };
    if !quiet {
        print_plan(&plan, &platform);
    }
    if !args.yes && !interactive {
        bail!("installation requires --yes when input is not interactive");
    }
    if !args.yes
        && !Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Install this environment?")
            .default(true)
            .interact()?
    {
        bail!("installation cancelled");
    }
    if plan.driver_action.is_some() {
        install_driver(&platform, args.yes)?;
    }
    match scope {
        InstallScope::User => install_user(plan, paths, &platform, &catalog, quiet),
        InstallScope::System => install_system(plan, paths, &platform),
    }
}

fn choose_profile() -> Result<Profile> {
    let choices = [
        "Runtime  · core runtime and math libraries · smallest",
        "Toolkit  · runtime + nvcc + headers + developer tools",
        "Full     · all supported non-driver toolkit components",
    ];
    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Choose an installation profile")
        .items(choices)
        .default(1)
        .interact()?;
    Ok([Profile::Runtime, Profile::Toolkit, Profile::Full][index])
}

fn print_plan(plan: &InstallPlan, platform: &Platform) {
    let size = human_bytes(plan.download_bytes);
    println!("\n\x1b[1;36m┌─ CURA · Installation plan ─────────────────────────────\x1b[0m");
    println!("│ CUDA       {}", plan.release);
    println!("│ Platform   {}", platform.display_name());
    println!("│ Scope      {:?}", plan.scope);
    println!(
        "│ Profile    {} · {}",
        plan.profile.label(),
        plan.profile.description()
    );
    if plan.scope == InstallScope::User {
        println!(
            "│ Payload    {} components · {size} download",
            plan.components.len()
        );
    } else {
        println!(
            "│ Backend    native {:?} packages",
            platform.package_manager
        );
    }
    println!(
        "│ Driver     {}",
        plan.driver_action
            .as_deref()
            .unwrap_or("compatible / no change")
    );
    println!("\x1b[1;36m└─────────────────────────────────────────────────────────\x1b[0m\n");
}

fn install_user(
    plan: InstallPlan,
    paths: &CuraPaths,
    platform: &Platform,
    catalog: &Catalog,
    quiet: bool,
) -> Result<EnvironmentManifest> {
    let target = paths.env_dir(&plan.release);
    if target.exists() {
        bail!(
            "cuda-{} is already installed at {}; remove it first",
            plan.release,
            target.display()
        );
    }
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.lock_file())?;
    lock.try_lock_exclusive()
        .context("another CURA installation is already running")?;
    let parent = paths.environments();
    let staging = tempfile::Builder::new()
        .prefix(".cura-staging-")
        .tempdir_in(&parent)?;
    let client = Client::builder()
        .user_agent(concat!("cura/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let progress = MultiProgress::new();
    for component in &plan.components {
        let archive = download_component(&client, catalog, component, paths, &progress)?;
        extract_component(&archive, staging.path())
            .with_context(|| format!("extract {}", component.name))?;
    }
    let manifest = EnvironmentManifest {
        release: plan.release,
        profile: plan.profile,
        scope: InstallScope::User,
        platform: platform.display_name(),
        installed_at_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        prefix: target.clone(),
        components: plan.components,
    };
    fs::write(
        staging.path().join(".cura-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let staging_path = staging.keep();
    fs::rename(&staging_path, &target)
        .with_context(|| format!("activate installation at {}", target.display()))?;
    if !quiet {
        println!(
            "\x1b[32m✓\x1b[0m Installed CUDA {} at {}",
            manifest.release,
            target.display()
        );
    }
    Ok(manifest)
}

fn download_component(
    client: &Client,
    _catalog: &Catalog,
    component: &Component,
    paths: &CuraPaths,
    progress: &MultiProgress,
) -> Result<std::path::PathBuf> {
    let filename = Path::new(&component.relative_path)
        .file_name()
        .context("component URL has no filename")?;
    let destination = paths.downloads().join(filename);
    if destination.is_file() && sha256_file(&destination)? == component.sha256 {
        return Ok(destination);
    }
    let partial = destination.with_extension("part");
    let url = Catalog::component_url(component);
    let mut response = client
        .get(&url)
        .send()?
        .error_for_status()
        .with_context(|| format!("download {url}"))?;
    let total = response.content_length().unwrap_or(component.size);
    let bar = progress.add(ProgressBar::new(total));
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} {msg:24} [{bar:30.cyan/blue}] {bytes}/{total_bytes} {eta}",
        )?
        .progress_chars("━━╸"),
    );
    bar.set_message(component.name.clone());
    let mut file = File::create(&partial)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
        bar.inc(count as u64);
    }
    file.sync_all()?;
    let actual = hex::encode(hasher.finalize());
    if actual != component.sha256 {
        let _ = fs::remove_file(&partial);
        bail!(
            "checksum mismatch for {}: expected {}, got {}",
            component.name,
            component.sha256,
            actual
        );
    }
    fs::rename(partial, &destination)?;
    bar.finish_with_message(format!("{} ✓", component.name));
    Ok(destination)
}

fn extract_component(archive: &Path, target: &Path) -> Result<()> {
    let file = File::open(archive)?;
    let decoder = XzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    for item in tar.entries()? {
        let mut entry = item?;
        let original = entry.path()?.into_owned();
        let mut parts = original.components();
        parts.next(); // NVIDIA archives have one component-specific top-level directory.
        let relative: std::path::PathBuf = parts.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| {
            matches!(
                c,
                PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
            )
        }) {
            bail!("unsafe archive path {}", original.display());
        }
        ensure_no_symlink_ancestors(target, &relative)?;
        if let Some(link) = entry.link_name()?
            && (link.is_absolute()
                || link
                    .components()
                    .any(|c| matches!(c, PathComponent::ParentDir | PathComponent::Prefix(_))))
        {
            bail!(
                "unsafe archive link {} -> {}",
                original.display(),
                link.display()
            );
        }
        entry.unpack(target.join(relative))?;
    }
    Ok(())
}

fn ensure_no_symlink_ancestors(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative
        .components()
        .take(relative.components().count().saturating_sub(1))
    {
        current.push(component);
        if current
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            bail!("archive entry would traverse symlink {}", current.display());
        }
    }
    Ok(())
}

fn install_system(
    plan: InstallPlan,
    paths: &CuraPaths,
    platform: &Platform,
) -> Result<EnvironmentManifest> {
    let manager = platform.package_manager.context("system installation is unsupported on this Linux distribution; use the default user-space scope")?;
    platform::ensure_cuda_repository(platform)?;
    let suffix = format!("{}-{}", plan.release.major, plan.release.minor);
    let package = match plan.profile {
        Profile::Runtime => format!("cuda-libraries-{suffix}"),
        Profile::Toolkit | Profile::Full => format!("cuda-toolkit-{suffix}"),
    };
    let (program, args): (&str, Vec<String>) = match manager {
        PackageManager::Apt => ("apt-get", vec!["install".into(), "-y".into(), package]),
        PackageManager::Dnf => ("dnf", vec!["install".into(), "-y".into(), package]),
        PackageManager::Zypper => (
            "zypper",
            vec!["--non-interactive".into(), "install".into(), package],
        ),
    };
    platform::run_privileged(program, &args).context("system package installation failed; ensure the official NVIDIA CUDA repository is configured for this distribution")?;
    let prefix = std::path::PathBuf::from(format!(
        "/usr/local/cuda-{}.{}",
        plan.release.major, plan.release.minor
    ));
    let manifest = EnvironmentManifest {
        release: plan.release,
        profile: plan.profile,
        scope: InstallScope::System,
        platform: platform.display_name(),
        installed_at_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        prefix,
        components: Vec::new(),
    };
    let record_dir = paths.data.join("system");
    fs::create_dir_all(&record_dir)?;
    fs::write(
        record_dir.join(format!("cuda-{}.json", manifest.release)),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

pub fn install_driver(platform: &Platform, yes: bool) -> Result<()> {
    platform.ensure_cuda_supported()?;
    if platform.wsl {
        bail!(
            "CURA never installs Linux GPU drivers inside WSL; update the NVIDIA driver on the Windows host"
        );
    }
    let manager = platform
        .package_manager
        .context("automatic driver installation requires apt, dnf, or zypper")?;
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if !yes && !interactive {
        bail!("driver installation requires --yes when input is not interactive");
    }
    if !yes && !Confirm::with_theme(&ColorfulTheme::default()).with_prompt("Install the latest NVIDIA driver from the configured repository? A reboot may be required").default(false).interact()? {
        bail!("driver installation cancelled");
    }
    platform::ensure_cuda_repository(platform)?;
    let (program, args): (&str, Vec<String>) = match manager {
        PackageManager::Apt => (
            "apt-get",
            vec!["install".into(), "-y".into(), "cuda-drivers".into()],
        ),
        PackageManager::Dnf => (
            "dnf",
            vec![
                "module".into(),
                "install".into(),
                "-y".into(),
                "nvidia-driver:latest-dkms".into(),
            ],
        ),
        PackageManager::Zypper => (
            "zypper",
            vec![
                "--non-interactive".into(),
                "install".into(),
                "cuda-drivers".into(),
            ],
        ),
    };
    platform::run_privileged(program, &args)?;
    println!("Driver packages installed. Reboot before using CUDA.");
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

pub fn human_bytes(value: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_sizes() {
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(0), "0 B");
    }
}
