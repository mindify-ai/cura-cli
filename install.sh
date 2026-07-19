#!/bin/sh
set -eu

repository="mindify-ai/cura-cli"
version="${CURA_VERSION:-latest}"
destination="${CURA_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
  *) echo "CURA release binaries support Linux x86_64 and aarch64" >&2; exit 1 ;;
esac

if [ "$version" = "latest" ]; then
  version=$(curl --proto '=https' --tlsv1.2 -fsSL "https://api.github.com/repos/$repository/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
fi
[ -n "$version" ] || { echo "Could not resolve the latest CURA release" >&2; exit 1; }

archive="cura-${version#v}-$target.tar.gz"
base="https://github.com/$repository/releases/download/$version"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

curl --proto '=https' --tlsv1.2 -fsSL "$base/$archive" -o "$tmp/$archive"
curl --proto '=https' --tlsv1.2 -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"
(cd "$tmp" && grep " $archive\$" SHA256SUMS | shasum -a 256 -c -)
tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$destination"
install -m 0755 "$tmp/cura" "$destination/cura"
echo "Installed CURA to $destination/cura"

