use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, theme::ColorfulTheme};
use serde::Serialize;
use serde_json::Value;

use crate::platform::Platform;

#[derive(Debug, Clone, Serialize)]
pub struct MetalStatus {
    pub supported: bool,
    pub ready: bool,
    pub gpu_name: Option<String>,
    pub metal_version: Option<String>,
    pub developer_dir: Option<String>,
    pub xcode_version: Option<String>,
    pub compiler_path: Option<String>,
    pub compiler_version: Option<String>,
    pub compiler_error: Option<String>,
}

pub fn status() -> MetalStatus {
    let platform = Platform::detect();
    if platform.os != "macos" {
        return MetalStatus {
            supported: false,
            ready: false,
            gpu_name: None,
            metal_version: None,
            developer_dir: None,
            xcode_version: None,
            compiler_path: None,
            compiler_version: None,
            compiler_error: None,
        };
    }

    let (gpu_name, metal_version) = metal_gpu();
    let developer_dir = successful_stdout(Command::new("xcode-select").arg("-p"));
    let xcode_version = successful_stdout(Command::new("xcodebuild").arg("-version"));
    let compiler_path = successful_stdout(Command::new("xcrun").args(["--find", "metal"]));
    let compiler = Command::new("xcrun").args(["metal", "--version"]).output();
    let (compiler_version, compiler_error) = match compiler {
        Ok(output) if output.status.success() => (output_text(&output), None),
        Ok(output) => (None, output_error(&output)),
        Err(error) => (None, Some(error.to_string())),
    };
    let ready = gpu_name.is_some() && compiler_version.is_some();

    MetalStatus {
        supported: true,
        ready,
        gpu_name,
        metal_version,
        developer_dir,
        xcode_version,
        compiler_path,
        compiler_version,
        compiler_error,
    }
}

pub fn install(yes: bool, interactive: bool, quiet: bool) -> Result<MetalStatus> {
    let platform = Platform::detect();
    platform.ensure_metal_supported()?;
    let before = status();
    if before.gpu_name.is_none() {
        bail!("no Metal-capable GPU was detected on this Mac");
    }
    if before.ready {
        if !quiet {
            println!(
                "\x1b[32m✓\x1b[0m Metal is ready on {}",
                before.gpu_name.as_deref().unwrap_or("this Mac")
            );
        }
        return Ok(before);
    }
    if before.xcode_version.is_none() {
        bail!(
            "Xcode is required to install the Metal toolchain. Install Xcode, then select it with `sudo xcode-select --switch /Applications/Xcode.app`"
        );
    }

    if !quiet {
        println!("\x1b[1;36mCURA · Metal installation plan\x1b[0m");
        println!(
            "  GPU         {}",
            before.gpu_name.as_deref().unwrap_or("Metal GPU")
        );
        println!(
            "  Runtime     {} (included with macOS)",
            before.metal_version.as_deref().unwrap_or("Metal")
        );
        println!("  Toolchain   Apple Metal compiler through Xcode");
        println!("  Command     xcodebuild -downloadComponent MetalToolchain");
    }
    if !yes && !interactive {
        bail!("Metal toolchain installation requires --yes when input is not interactive");
    }
    if !yes
        && !Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Download and install the Metal toolchain?")
            .default(true)
            .interact()?
    {
        bail!("installation cancelled");
    }

    let install = Command::new("xcodebuild")
        .args(["-downloadComponent", "MetalToolchain"])
        .status()
        .context("run xcodebuild -downloadComponent MetalToolchain")?;
    if !install.success() {
        bail!("xcodebuild failed to install the Metal toolchain ({install})");
    }
    let after = status();
    if !after.ready {
        bail!(
            "Xcode completed the download, but the Metal compiler is still unavailable: {}",
            after
                .compiler_error
                .as_deref()
                .unwrap_or("xcrun metal --version failed")
        );
    }
    if !quiet {
        println!(
            "\x1b[32m✓\x1b[0m Metal is ready on {}",
            after.gpu_name.as_deref().unwrap_or("this Mac")
        );
    }
    Ok(after)
}

pub fn print_status(status: &MetalStatus) {
    println!("\x1b[1;36mCURA Metal\x1b[0m");
    if !status.supported {
        println!("  \x1b[33m!\x1b[0m Platform         Metal requires macOS");
        return;
    }
    status_line(
        status.gpu_name.is_some(),
        "GPU",
        status.gpu_name.as_deref().unwrap_or("not detected"),
    );
    status_line(
        status.metal_version.is_some(),
        "Runtime",
        status.metal_version.as_deref().unwrap_or("not reported"),
    );
    status_line(
        status.xcode_version.is_some(),
        "Xcode",
        status.xcode_version.as_deref().unwrap_or("not detected"),
    );
    status_line(
        status.compiler_version.is_some(),
        "Compiler",
        status
            .compiler_version
            .as_deref()
            .or(status.compiler_error.as_deref())
            .unwrap_or("not detected"),
    );
    if !status.ready {
        println!("  Next step: cura metal install");
    }
}

fn metal_gpu() -> (Option<String>, Option<String>) {
    let Ok(output) = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
    else {
        return (None, None);
    };
    if !output.status.success() {
        return (None, None);
    }
    parse_system_profiler(&output.stdout)
}

fn parse_system_profiler(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return (None, None);
    };
    let Some(displays) = value.get("SPDisplaysDataType").and_then(Value::as_array) else {
        return (None, None);
    };
    displays
        .iter()
        .find_map(|display| {
            let raw_version = display
                .get("spdisplays_mtlgpufamilysupport")
                .and_then(Value::as_str)?;
            let name = display
                .get("sppci_model")
                .or_else(|| display.get("_name"))?;
            Some((
                name.as_str().map(str::to_owned),
                Some(format_metal_version(raw_version)),
            ))
        })
        .unwrap_or((None, None))
}

fn format_metal_version(raw: &str) -> String {
    let normalized = raw
        .strip_prefix("spdisplays_metal")
        .or_else(|| raw.strip_prefix("Metal "));
    normalized
        .filter(|version| !version.is_empty())
        .map(|version| format!("Metal {version}"))
        .unwrap_or_else(|| raw.to_string())
}

fn successful_stdout(command: &mut Command) -> Option<String> {
    command.output().ok().and_then(|output| {
        output
            .status
            .success()
            .then(|| output_text(&output))
            .flatten()
    })
}

fn output_text(output: &Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

fn output_error(output: &Output) -> Option<String> {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stderr.is_empty())
        .then_some(stderr)
        .or_else(|| (!stdout.is_empty()).then_some(stdout))
}

fn status_line(ok: bool, label: &str, value: &str) {
    let marker = if ok {
        "\x1b[32m✓\x1b[0m"
    } else {
        "\x1b[33m!\x1b[0m"
    };
    println!("  {marker} {label:<16} {value}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apple_gpu_and_metal_version() {
        let json = br#"{
          "SPDisplaysDataType": [{
            "_name": "Apple M2",
            "spdisplays_mtlgpufamilysupport": "spdisplays_metal4",
            "sppci_model": "Apple M2"
          }]
        }"#;
        assert_eq!(
            parse_system_profiler(json),
            (Some("Apple M2".into()), Some("Metal 4".into()))
        );
    }

    #[test]
    fn ignores_display_entries_without_metal_support() {
        let json = br#"{"SPDisplaysDataType":[{"_name":"External Display"}]}"#;
        assert_eq!(parse_system_profiler(json), (None, None));
    }
}
