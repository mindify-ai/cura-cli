#!/bin/sh
set -eu

repository="https://github.com/mindify-ai/cura-cli.git"
branch="${CURA_REF:-main}"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
destination="${CURA_INSTALL_DIR:-$cargo_home/bin}"
original_path="$PATH"

say() {
  printf '%s\n' "$*"
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

command -v uname >/dev/null 2>&1 || fail "uname is required"
command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v mktemp >/dev/null 2>&1 || fail "mktemp is required"

os=$(uname -s)
arch=$(uname -m)

case "$os-$arch" in
  Linux-x86_64|Linux-amd64|Linux-aarch64|Linux-arm64)
    environment="Linux ($arch)"
    if [ -r /proc/sys/kernel/osrelease ] && grep -qi microsoft /proc/sys/kernel/osrelease; then
      environment="WSL ($arch)"
    fi
    ;;
  Darwin-x86_64|Darwin-arm64|Darwin-aarch64)
    environment="macOS ($arch)"
    ;;
  *)
    fail "unsupported environment: $os ($arch); CURA supports Linux, WSL, and macOS on x86_64 or arm64"
    ;;
esac

say "Detected $environment"

CARGO_HOME="$cargo_home"
export CARGO_HOME
PATH="$cargo_home/bin:$PATH"
export PATH

if [ -f "$cargo_home/env" ]; then
  # rustup writes this POSIX-compatible environment file.
  . "$cargo_home/env"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

if ! command -v rustc >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
  say "Rust and Cargo were not found; installing the current stable toolchain with rustup..."
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$tmp/rustup-init.sh"
  sh "$tmp/rustup-init.sh" -y --profile minimal --default-toolchain stable

  [ -f "$cargo_home/env" ] || fail "rustup finished without creating $cargo_home/env"
  . "$cargo_home/env"
fi

command -v rustc >/dev/null 2>&1 || fail "Rust installation did not provide rustc"
command -v cargo >/dev/null 2>&1 || fail "Rust installation did not provide Cargo"
command -v install >/dev/null 2>&1 || fail "the POSIX install utility is required"

say "Using $(rustc --version)"
say "Using $(cargo --version)"
say "Installing CURA from $repository ($branch)..."

cargo install \
  --git "$repository" \
  --branch "$branch" \
  --locked \
  --root "$tmp/cura-install" \
  cura

mkdir -p "$destination"
install -m 0755 "$tmp/cura-install/bin/cura" "$destination/cura"

say "Installed CURA to $destination/cura"
case ":$original_path:" in
  *":$destination:"*) ;;
  *) say "Add $destination to PATH to run 'cura' from a new shell." ;;
esac
