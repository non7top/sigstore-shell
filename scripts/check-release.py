#!/usr/bin/env python3
"""Checks that every built file carries the release version and the commit it was built from.

usage: check-release.py <version> <commit> <dist-dir>
"""
import re
import subprocess
import sys
from pathlib import Path


def utf16(text):
    return text.encode("utf-16-le")


def has_resource_string(data, key, value):
    # VS_VERSION_INFO string entry: UTF-16 key, NUL, 0 or 2 bytes of padding, UTF-16 value, NUL.
    pattern = re.escape(utf16(key)) + rb"\x00\x00(?:\x00\x00)?" + re.escape(utf16(value)) + rb"\x00\x00"
    return re.search(pattern, data) is not None


def main():
    version, commit, dist = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    failures = []

    linux = dist / "sigstore-shell-cli"
    want = f"sigstore-shell-cli {version} (commit {commit})"
    got = subprocess.run([str(linux), "--version"], capture_output=True, text=True, check=True).stdout.strip()
    print(f"{linux.name}: {got}")
    if got != want:
        failures.append(f"{linux.name} printed {got!r}, expected {want!r}")

    windows = [dist / "sigstore-shell-cli.exe", dist / "sigstore_shell_ext.dll"]
    windows += sorted(dist.glob("sigstore-shell-Setup-*.exe"))
    if len(windows) != 3:
        failures.append(f"expected the CLI, the DLL and one installer, found {[p.name for p in windows]}")
    for path in windows:
        data = path.read_bytes()
        for key, value in (("FileVersion", version), ("ProductVersion", version), ("BuildCommit", commit)):
            ok = has_resource_string(data, key, value)
            print(f"{path.name}: {key} = {value}: {'ok' if ok else 'MISSING'}")
            if not ok:
                failures.append(f"{path.name} has no {key} = {value} in its version resource")

    for failure in failures:
        print(f"::error::{failure}")
    return 1 if failures else 0


sys.exit(main())
