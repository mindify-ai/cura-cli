use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use semver::Version;

use crate::{
    catalog::Catalog,
    cli::{
        Cli, Command, DriverCommand, EnvArgs, ListArgs, MetalCommand, RemoveArgs, RunArgs,
        ShellCommand, ShellKind, UseArgs,
    },
    install, metal,
    model::{EnvironmentManifest, InstallScope, JsonEnvelope, VersionSpec},
    paths::{CuraPaths, find_project_version},
    platform::{self, PackageManager, Platform},
    tui::{self, DashboardAction, DashboardKind},
};

pub fn run(cli: Cli) -> Result<()> {
    let paths = CuraPaths::discover()?;
    paths.ensure()?;
    let interactive = !cli.no_interactive
        && !cli.json
        && std::io::IsTerminal::is_terminal(&std::io::stdin())
        && std::io::IsTerminal::is_terminal(&std::io::stdout());
    match cli.command {
        None if interactive => run_dashboard(&paths),
        None => {
            print_help_hint();
            Ok(())
        }
        Some(Command::Install(args)) if args.version.eq_ignore_ascii_case("metal") => {
            if args.profile.is_some() || args.system || args.no_driver {
                bail!("--profile, --system, and --no-driver apply only to CUDA installations");
            }
            let status = metal::install(args.yes, interactive, cli.json)?;
            if cli.json {
                print_json(&status)?;
            }
            Ok(())
        }
        Some(Command::Install(args)) => {
            let manifest = install::install(args, &paths, interactive, cli.json)?;
            if cli.json {
                print_json(&manifest)?;
            }
            Ok(())
        }
        Some(Command::List(args)) => list(args, &paths, cli.json),
        Some(Command::Use(args)) => use_version(args, &paths, cli.json),
        Some(Command::Run(args)) => run_in_environment(args, &paths),
        Some(Command::Env(args)) => print_env(args, &paths, cli.json),
        Some(Command::Remove(args)) => remove(args, &paths, interactive),
        Some(Command::Doctor) => doctor(&paths, cli.json),
        Some(Command::Driver(args)) => match args.command {
            DriverCommand::Status => driver_status(cli.json),
            DriverCommand::Install { yes } => install::install_driver(&Platform::detect(), yes),
        },
        Some(Command::Metal(args)) => match args.command {
            MetalCommand::Status => metal_status(cli.json),
            MetalCommand::Install { yes } => {
                let status = metal::install(yes, interactive, cli.json)?;
                if cli.json {
                    print_json(&status)?;
                }
                Ok(())
            }
        },
        Some(Command::Shell(args)) => match args.command {
            ShellCommand::Init {
                shell,
                write,
                rc_file,
            } => shell_init(shell, write, rc_file),
        },
    }
}

fn run_dashboard(paths: &CuraPaths) -> Result<()> {
    loop {
        let platform = Platform::detect();
        let kind = if platform.os == "macos" {
            DashboardKind::Metal
        } else {
            DashboardKind::Cuda
        };
        let environments = installed(paths)?;
        let active = selected_raw(paths).ok().flatten();
        match tui::dashboard(kind, environments.len(), active.as_deref())? {
            DashboardAction::Install => {
                if kind == DashboardKind::Metal {
                    metal::install(false, true, false)?;
                } else {
                    let version: String = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("CUDA version")
                        .default("cuda-12".into())
                        .interact_text()?;
                    install::install(
                        crate::cli::InstallArgs {
                            version,
                            profile: None,
                            system: false,
                            yes: false,
                            no_driver: false,
                        },
                        paths,
                        true,
                        false,
                    )?;
                }
            }
            DashboardAction::Environments if kind == DashboardKind::Metal => metal_status(false)?,
            DashboardAction::Environments => list(ListArgs { available: false }, paths, false)?,
            DashboardAction::Use => {
                let envs = installed(paths)?;
                if envs.is_empty() {
                    println!("No CUDA environments installed.");
                    continue;
                }
                let labels: Vec<_> = envs
                    .iter()
                    .map(|e| format!("cuda-{} · {} · {:?}", e.release, e.profile, e.scope))
                    .collect();
                let choice = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Select CUDA environment")
                    .items(&labels)
                    .interact()?;
                use_version(
                    UseArgs {
                        version: format!("cuda-{}", envs[choice].release),
                        global: false,
                        local: true,
                    },
                    paths,
                    false,
                )?;
            }
            DashboardAction::Doctor => doctor(paths, false)?,
            DashboardAction::Quit => return Ok(()),
        }
        println!("\nPress Enter to return to CURA…");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

fn list(args: ListArgs, paths: &CuraPaths, json: bool) -> Result<()> {
    if args.available {
        let mut versions = Catalog::new(paths)?.versions()?;
        versions.reverse();
        if json {
            return print_json(&versions);
        }
        println!("\x1b[1mAvailable CUDA releases\x1b[0m");
        for version in versions {
            println!("  cuda-{version}");
        }
        return Ok(());
    }
    let envs = installed(paths)?;
    if json {
        return print_json(&envs);
    }
    if envs.is_empty() {
        if Platform::detect().os == "macos" {
            println!("Metal is managed by macOS. Run: cura metal status");
        } else {
            println!("No CUDA environments installed. Try: cura install cuda-12");
        }
        return Ok(());
    }
    let selected = selected_raw(paths)?.unwrap_or_default();
    println!("\x1b[1mInstalled CUDA environments\x1b[0m");
    for item in envs {
        let marker = if selected_version_matches(&selected, &item.release) {
            "\x1b[32m●\x1b[0m"
        } else {
            "○"
        };
        println!(
            "  {marker} cuda-{:<10} {:<8} {:?}  {}",
            item.release,
            item.profile,
            item.scope,
            item.prefix.display()
        );
    }
    Ok(())
}

fn use_version(args: UseArgs, paths: &CuraPaths, json: bool) -> Result<()> {
    let manifest = resolve_installed(&args.version, paths)?;
    let destination = if args.global {
        paths.global_version_file()
    } else {
        env::current_dir()?.join(".cura-version")
    };
    fs::write(&destination, format!("cuda-{}\n", manifest.release))
        .with_context(|| format!("write {}", destination.display()))?;
    if json {
        print_json(
            &serde_json::json!({"version": manifest.release, "scope": if args.global {"global"} else {"local"}, "file": destination}),
        )
    } else {
        println!(
            "\x1b[32m✓\x1b[0m Using CUDA {} {}",
            manifest.release,
            if args.global {
                "globally"
            } else {
                "in this project"
            }
        );
        Ok(())
    }
}

fn run_in_environment(args: RunArgs, paths: &CuraPaths) -> Result<()> {
    let manifest = match args.cuda {
        Some(v) => resolve_installed(&v, paths)?,
        None => selected(paths)?,
    };
    let mut command = ProcessCommand::new(&args.command[0]);
    command.args(&args.command[1..]);
    apply_environment(&mut command, &manifest.prefix);
    let status = command
        .status()
        .with_context(|| format!("run {}", args.command[0]))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn print_env(args: EnvArgs, paths: &CuraPaths, json: bool) -> Result<()> {
    let manifest = selected(paths)?;
    if json {
        return print_json(
            &serde_json::json!({"cuda_home": manifest.prefix, "version": manifest.release}),
        );
    }
    let prefix = manifest.prefix.display();
    match args.shell.or_else(detect_shell).unwrap_or(ShellKind::Bash) {
        ShellKind::Fish => println!(
            "set -gx CUDA_HOME '{prefix}';\nset -gx PATH '{prefix}/bin' $PATH;\nset -gx LD_LIBRARY_PATH '{prefix}/lib64' '{prefix}/lib' $LD_LIBRARY_PATH;"
        ),
        ShellKind::Bash | ShellKind::Zsh => println!(
            "export CUDA_HOME='{prefix}'\nexport PATH='{prefix}/bin:'\"$PATH\"\nexport LD_LIBRARY_PATH='{prefix}/lib64:{prefix}/lib:'\"${{LD_LIBRARY_PATH:-}}\""
        ),
    }
    Ok(())
}

fn remove(args: RemoveArgs, paths: &CuraPaths, interactive: bool) -> Result<()> {
    let manifest = resolve_installed(&args.version, paths)?;
    if !args.yes && !interactive {
        bail!("removal requires --yes when input is not interactive");
    }
    if args.system || manifest.scope == InstallScope::System {
        let platform = Platform::detect();
        let manager = platform
            .package_manager
            .context("cannot remove system CUDA packages on this distribution")?;
        let suffix = format!("{}-{}", manifest.release.major, manifest.release.minor);
        let package = match manifest.profile {
            crate::model::Profile::Runtime => format!("cuda-libraries-{suffix}"),
            _ => format!("cuda-toolkit-{suffix}"),
        };
        if !args.yes
            && interactive
            && !Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Remove system package {package}?"))
                .default(false)
                .interact()?
        {
            bail!("removal cancelled");
        }
        let (program, command_args) = match manager {
            PackageManager::Apt => ("apt-get", vec!["remove".into(), "-y".into(), package]),
            PackageManager::Dnf => ("dnf", vec!["remove".into(), "-y".into(), package]),
            PackageManager::Zypper => (
                "zypper",
                vec!["--non-interactive".into(), "remove".into(), package],
            ),
        };
        platform::run_privileged(program, &command_args)?;
        let record = paths
            .data
            .join("system")
            .join(format!("cuda-{}.json", manifest.release));
        let _ = fs::remove_file(record);
    } else {
        if !args.yes
            && interactive
            && !Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(format!(
                    "Remove cuda-{} and all its files?",
                    manifest.release
                ))
                .default(false)
                .interact()?
        {
            bail!("removal cancelled");
        }
        fs::remove_dir_all(&manifest.prefix)
            .with_context(|| format!("remove {}", manifest.prefix.display()))?;
    }
    println!("\x1b[32m✓\x1b[0m Removed CUDA {}", manifest.release);
    Ok(())
}

fn doctor(paths: &CuraPaths, json: bool) -> Result<()> {
    let platform = Platform::detect();
    let driver = platform::driver_status(&platform);
    let metal = (platform.os == "macos").then(metal::status);
    let envs = installed(paths)?;
    let selected = selected(paths).ok();
    let report = serde_json::json!({
        "platform": platform,
        "driver": driver,
        "metal": metal,
        "accelerator": if platform.os == "macos" { "metal" } else { "cuda" },
        "installed_environments": envs.len(),
        "selected": selected.as_ref().map(|e| format!("cuda-{}", e.release)),
        "shell_integration": env::var_os("CURA_SHELL_INTEGRATION").is_some(),
    });
    if json {
        return print_json(&report);
    }
    println!("\x1b[1;36mCURA doctor\x1b[0m");
    if let Some(metal) = metal {
        check_line(
            metal.supported,
            "Platform",
            &platform.display_name(),
            "Metal requires macOS",
        );
        check_line(
            metal.gpu_name.is_some(),
            "Metal GPU",
            metal.gpu_name.as_deref().unwrap_or("not detected"),
            "this Mac does not report a Metal-capable GPU",
        );
        check_line(
            metal.metal_version.is_some(),
            "Metal runtime",
            metal.metal_version.as_deref().unwrap_or("not reported"),
            "update macOS",
        );
        check_line(
            metal.compiler_version.is_some(),
            "Metal compiler",
            metal
                .compiler_version
                .as_deref()
                .or(metal.compiler_error.as_deref())
                .unwrap_or("not detected"),
            "run cura metal install",
        );
        return Ok(());
    }
    check_line(
        platform.os == "linux",
        "Platform",
        &platform.display_name(),
        "CUDA installation requires Linux or WSL",
    );
    check_line(
        driver.version.is_some(),
        "NVIDIA driver",
        driver.version.as_deref().unwrap_or("not detected"),
        if platform.wsl {
            "update the Windows host driver"
        } else {
            "run cura driver install"
        },
    );
    check_line(
        !envs.is_empty(),
        "Environments",
        &format!("{} installed", envs.len()),
        "run cura install cuda-12",
    );
    check_line(
        selected.is_some(),
        "Selection",
        &selected
            .map(|e| format!("cuda-{}", e.release))
            .unwrap_or_else(|| "none".into()),
        "run cura use <version> --global",
    );
    check_line(
        env::var_os("CURA_SHELL_INTEGRATION").is_some(),
        "Shell hook",
        "loaded",
        "add eval \"$(cura shell init zsh)\" to your shell rc",
    );
    Ok(())
}

fn driver_status(json: bool) -> Result<()> {
    let platform = Platform::detect();
    let status = platform::driver_status(&platform);
    if json {
        print_json(&status)
    } else {
        println!("GPU present: {}", status.gpu_present);
        println!(
            "Driver: {}",
            status.version.as_deref().unwrap_or("not detected")
        );
        if status.wsl_managed {
            println!("Driver ownership: Windows host (CURA will not modify it)");
        }
        Ok(())
    }
}

fn metal_status(json: bool) -> Result<()> {
    let status = metal::status();
    if json {
        print_json(&status)
    } else {
        metal::print_status(&status);
        Ok(())
    }
}

fn shell_init(shell: ShellKind, write: bool, rc_file: Option<PathBuf>) -> Result<()> {
    let script = match shell {
        ShellKind::Bash => {
            "export CURA_SHELL_INTEGRATION=1\n_cura_update() { eval \"$(command cura env --shell bash 2>/dev/null)\" || true; }\nPROMPT_COMMAND=\"_cura_update${PROMPT_COMMAND:+;$PROMPT_COMMAND}\""
        }
        ShellKind::Zsh => {
            "export CURA_SHELL_INTEGRATION=1\n_cura_update() { eval \"$(command cura env --shell zsh 2>/dev/null)\" || true; }\nautoload -Uz add-zsh-hook\nadd-zsh-hook chpwd _cura_update\n_cura_update"
        }
        ShellKind::Fish => {
            "set -gx CURA_SHELL_INTEGRATION 1\nfunction _cura_update --on-variable PWD\n  command cura env --shell fish 2>/dev/null | source\nend\n_cura_update"
        }
    };
    if !write {
        println!("{script}");
        return Ok(());
    }
    let rc = rc_file.unwrap_or_else(|| default_rc(shell));
    let marker = format!("\n# CURA shell integration\n{script}\n");
    let current = fs::read_to_string(&rc).unwrap_or_default();
    if current.contains("# CURA shell integration") {
        println!("CURA shell integration already exists in {}", rc.display());
        return Ok(());
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&rc)?
        .write_all(marker.as_bytes())?;
    println!(
        "\x1b[32m✓\x1b[0m Added CURA shell integration to {}",
        rc.display()
    );
    Ok(())
}

fn installed(paths: &CuraPaths) -> Result<Vec<EnvironmentManifest>> {
    let mut result: Vec<EnvironmentManifest> = Vec::new();
    for root in [paths.environments(), paths.data.join("system")] {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = if entry.path().is_dir() {
                entry.path().join(".cura-manifest.json")
            } else {
                entry.path()
            };
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = fs::read(&path)
                && let Ok(manifest) = serde_json::from_slice(&bytes)
            {
                result.push(manifest);
            }
        }
    }
    result.sort_by(|a, b| b.release.cmp(&a.release));
    Ok(result)
}

fn resolve_installed(raw: &str, paths: &CuraPaths) -> Result<EnvironmentManifest> {
    let spec: VersionSpec = raw.parse()?;
    installed(paths)?
        .into_iter()
        .filter(|e| spec.matches(&e.release))
        .max_by(|a, b| a.release.cmp(&b.release))
        .with_context(|| format!("{raw} is not installed; run cura install {raw}"))
}

fn selected(paths: &CuraPaths) -> Result<EnvironmentManifest> {
    let raw = selected_raw(paths)?.context(
        "no CUDA version selected; run cura use <version> --global or create .cura-version",
    )?;
    resolve_installed(&raw, paths)
}

fn selected_raw(paths: &CuraPaths) -> Result<Option<String>> {
    let local = find_project_version(&env::current_dir()?);
    let path = local.or_else(|| {
        paths
            .global_version_file()
            .is_file()
            .then(|| paths.global_version_file())
    });
    Ok(path
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

fn selected_version_matches(raw: &str, version: &Version) -> bool {
    raw.parse::<VersionSpec>().is_ok_and(|s| s.matches(version))
}

fn apply_environment(command: &mut ProcessCommand, prefix: &Path) {
    let old_path = env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![prefix.join("bin")];
    paths.extend(env::split_paths(&old_path));
    command
        .env("CUDA_HOME", prefix)
        .env("PATH", env::join_paths(paths).unwrap());
    let old_lib = env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
    let mut libs = vec![prefix.join("lib64"), prefix.join("lib")];
    libs.extend(env::split_paths(&old_lib));
    command.env("LD_LIBRARY_PATH", env::join_paths(libs).unwrap());
}

fn check_line(ok: bool, label: &str, value: &str, advice: &str) {
    if ok {
        println!("  \x1b[32m✓\x1b[0m {label:<16} {value}");
    } else {
        println!("  \x1b[33m!\x1b[0m {label:<16} {value} · {advice}");
    }
}

fn print_json<T: serde::Serialize>(data: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&JsonEnvelope {
            status: "ok",
            data,
            warnings: Vec::new()
        })?
    );
    Ok(())
}

fn detect_shell() -> Option<ShellKind> {
    let shell = env::var("SHELL").ok()?;
    if shell.ends_with("zsh") {
        Some(ShellKind::Zsh)
    } else if shell.ends_with("fish") {
        Some(ShellKind::Fish)
    } else {
        Some(ShellKind::Bash)
    }
}

fn default_rc(shell: ShellKind) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    match shell {
        ShellKind::Bash => home.join(".bashrc"),
        ShellKind::Zsh => home.join(".zshrc"),
        ShellKind::Fish => home.join(".config/fish/config.fish"),
    }
}

fn print_help_hint() {
    println!(
        "CURA needs an interactive terminal for the dashboard. Run `cura --help` for commands."
    );
}
