# CURA

**CURA** is an interactive CUDA environment manager for Linux and WSL, written in Rust.
It installs versioned CUDA environments side by side, switches them per project, verifies
every downloaded component, and can safely coordinate native package and driver changes.

```console
$ cura install cuda-12
┌─ CURA · Installation plan ─────────────────────────────
│ CUDA       12.9.2
│ Scope      User
│ Profile    Toolkit · runtime, nvcc, headers, and developer tools
│ Payload    27 components · 3.4 GiB download
│ Driver     compatible / no change
└─────────────────────────────────────────────────────────
```

Running `cura` opens the full-screen dashboard. The same workflows remain available as
regular subcommands for scripts and CI.

## Features

- Resolve aliases such as `cuda-12` and pin the exact newest published patch release.
- Compare Runtime, Toolkit, and Full profiles in an interactive installer.
- Install without sudo under the XDG data directory, with SHA-256 verification and atomic activation.
- Optionally install through apt, dnf, or zypper using NVIDIA's official repositories.
- Detect incompatible drivers and review upgrades before sudo; never install a driver inside WSL.
- Select CUDA globally or through a project-local `.cura-version` file.
- Run commands in an environment without changing the parent shell.
- Diagnose the platform, GPU driver, installed environments, selection, and shell hook.

## Install CURA

Build from source with a current stable Rust toolchain:

```sh
cargo install --path .
```

Prebuilt release binaries can be installed with:

```sh
curl --proto '=https' --tlsv1.2 -fsSL \
  https://raw.githubusercontent.com/mindify-ai/cura-cli/main/install.sh | sh
```

CUDA itself is supported on native Linux and WSL. The CURA binary builds on macOS for
development and reports an actionable unsupported-platform error for install operations.

## Usage

```sh
# Interactive wizard; choose the payload after seeing its size.
cura install cuda-12

# Deterministic CI installation.
cura install cuda-12.9.2 --profile toolkit --yes --no-interactive

# Privileged native package installation.
cura install cuda-12 --profile toolkit --system

cura list
cura list --available
cura use cuda-12                  # writes .cura-version
cura use cuda-12 --global
cura run --cuda cuda-12 -- nvcc --version
cura doctor
cura remove cuda-12
```

Enable automatic environment updates when changing directories:

```sh
# zsh
eval "$(cura shell init zsh)"

# bash
eval "$(cura shell init bash)"

# fish
cura shell init fish | source
```

Use `cura shell init zsh --write` (or `bash`/`fish`) to append the hook after reviewing it.

## Data and security

CURA follows XDG paths for configuration, data, cache, and state. Set `CURA_HOME` to keep
all four beneath one directory. User installs are downloaded directly from NVIDIA's
redistributable catalog, checked against the manifest SHA-256, safely extracted into a
staging directory, and atomically activated. Partial downloads remain reusable; partial
environments never become selectable.

System and driver operations display a plan before invoking `sudo`. In non-interactive
contexts, payload selection and destructive confirmation must be explicit.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

