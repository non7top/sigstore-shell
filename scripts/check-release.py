#!/usr/bin/env python3
"""Checks that every built file carries the release version and the commit it was built from.

usage: check-release.py <version> <commit> <dist-dir> [<owner/repo>]

With a repo, each file must also embed the link to the cosign bundle of its own release asset, and each
Windows file must name the repo as its CompanyName.
"""
import re
import struct
import subprocess
import sys
from pathlib import Path


def utf16(text):
    return text.encode("utf-16-le")


def has_resource_string(data, key, value):
    # VS_VERSION_INFO string entry: UTF-16 key, NUL, 0 or 2 bytes of padding, UTF-16 value, NUL.
    pattern = re.escape(utf16(key)) + rb"\x00\x00(?:\x00\x00)?" + re.escape(utf16(value)) + rb"\x00\x00"
    return re.search(pattern, data) is not None


def fixed_versions(data):
    # VS_FIXEDFILEINFO: signature 0xFEEF04BD, struct version, then file and product version as MS/LS words.
    at = data.find(b"\xbd\x04\xef\xfe")
    if at < 0 or len(data) < at + 20:
        return None
    file_ms, file_ls, prod_ms, prod_ls = struct.unpack_from("<4I", data, at + 8)
    fmt = lambda ms, ls: f"{ms >> 16}.{ms & 0xFFFF}.{ls >> 16}.{ls & 0xFFFF}"
    return fmt(file_ms, file_ls), fmt(prod_ms, prod_ls)


def main():
    version, commit, dist = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    repo = sys.argv[4] if len(sys.argv) > 4 else None

    def bundle_url(asset):
        return f"https://github.com/{repo}/releases/download/v{version}/{asset}.cosign.bundle"
    failures = []

    linux = dist / "sigstore-shell-cli"
    want = f"sigstore-shell-cli {version} (commit {commit})"
    got = subprocess.run([str(linux), "--version"], capture_output=True, text=True, check=True).stdout.strip()
    print(f"{linux.name}: {got}")
    if got != want:
        failures.append(f"{linux.name} printed {got!r}, expected {want!r}")
    if repo:
        url = bundle_url(linux.name)
        ok = url.encode() in linux.read_bytes()
        print(f"{linux.name}: ProvenanceBundle = {url}: {'ok' if ok else 'MISSING'}")
        if not ok:
            failures.append(f"{linux.name} does not embed {url}")

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
        numeric = re.match(r"(\d+)\.(\d+)\.(\d+)", version)
        if numeric:
            want = ".".join(numeric.groups()) + ".0"
            got = fixed_versions(data)
            for label, value in zip(("FILEVERSION", "PRODUCTVERSION"), got or (None, None)):
                ok = value == want
                print(f"{path.name}: numeric {label} = {want}: {'ok' if ok else 'MISSING (' + str(value) + ')'}")
                if not ok:
                    failures.append(f"{path.name} has numeric {label} {value}, expected {want}")
        if repo:
            ok = has_resource_string(data, "CompanyName", repo)
            print(f"{path.name}: CompanyName = {repo}: {'ok' if ok else 'MISSING'}")
            if not ok:
                failures.append(f"{path.name} has no CompanyName = {repo} in its version resource")
            url = bundle_url(path.name)
            ok = has_resource_string(data, "ProvenanceBundle", url)
            print(f"{path.name}: ProvenanceBundle = {url}: {'ok' if ok else 'MISSING'}")
            if not ok:
                failures.append(f"{path.name} has no ProvenanceBundle = {url} in its version resource")

    for failure in failures:
        print(f"::error::{failure}")
    return 1 if failures else 0


sys.exit(main())
