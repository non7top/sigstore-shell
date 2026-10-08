#!/usr/bin/env bash
# Builds dist/sigstore-shell-Setup-<version>.exe around the already-built dist/sigstore_shell_ext.dll.
set -euo pipefail

version=${1:?usage: installer.sh <version>}
cd "$(dirname "$0")/.."

# The numeric resource field needs x.y.z.w; a PR build's "0.2.0-rc5" or a bare sha falls back to what it can.
if [[ $version =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    numeric="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}.0"
else
    numeric=0.0.0.0
fi

args=(-V2 "-DVERSION=$version" "-DVERSION_NUMERIC=$numeric" "-DOUTFILE=../dist/sigstore-shell-Setup-$version.exe")
if [ -n "${PROVENANCE_REPO:-}" ]; then
    args+=("-DPROVENANCE_REPO=$PROVENANCE_REPO")
fi
makensis "${args[@]}" installer/sigstore-shell.nsi
