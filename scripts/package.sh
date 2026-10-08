#!/usr/bin/env bash
# Zips the already-built DLL with its installer files and writes SHA256SUMS over the built files in dist/.
set -euo pipefail

version=${1:?usage: package.sh <version>}
cd "$(dirname "$0")/.."

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp dist/sigstore_shell_ext.dll installer/register.ps1 installer/unregister.ps1 installer/INSTALL.md "$stage/"

zip_name="sigstore-shell-ext-$version-windows-x64.zip"
rm -f "dist/$zip_name"
(cd "$stage" && zip -X -q "$OLDPWD/dist/$zip_name" ./*)

cd dist
rm -f SHA256SUMS
sha256sum sigstore_shell_ext.dll sigstore-shell-cli.exe sigstore-shell-cli "$zip_name" "sigstore-shell-Setup-$version.exe" > SHA256SUMS
