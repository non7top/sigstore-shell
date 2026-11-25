#!/usr/bin/env bash
# Builds the release binaries into dist/: the DLL and the Windows CLI in one cargo call, so they share compiled dependencies, then the Linux CLI.
set -euo pipefail

cd "$(dirname "$0")/.."
win=x86_64-pc-windows-gnu
target_dir=${CARGO_TARGET_DIR:-target}

cargo build --release --locked --target "$win" -p sigstore-shell-ext -p sigstore-shell-cli
cargo build --release --locked -p sigstore-shell-cli

mkdir -p dist
cp "$target_dir/$win/release/sigstore_shell_ext.dll" "$target_dir/$win/release/sigstore-shell-cli.exe" "$target_dir/release/sigstore-shell-cli" dist/
