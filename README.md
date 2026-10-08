# sigstore-shell

A Windows Explorer extension that adds a **Sigstore** tab to a file's Properties dialog. It shows which GitHub repo built an exe, with the workflow, commit and signing date, verified from the file's Sigstore attestation.

Status: early development. The command-line prototype and the Explorer extension (below) are built; the extension has not yet been run in real Explorer (see [Verification status](#verification-status)).

## How it works

1. The publishing project embeds `owner/repo` in the file (a `ProvenanceRepo` version-resource string in a PE, an ELF note in a Linux binary; format in [project.md](project.md#embedded-claim)) and attests the exe with `actions/attest-build-provenance`.
2. The tab shows that string as an unverified claim. Pressing **Verify** hashes the file, fetches the attestation for that digest from GitHub, checks it against Sigstore's roots and shows the identity from the certificate.

The embedded string is only a hint. The identity shown always comes from the certificate, and a mismatch is flagged. Nothing is hashed or sent over the network until you press Verify.

Optionally, Rekor v1 can be searched by hash for files with no embedded repo. It is off by default because it sends the hash to a second service.

See [project.md](project.md) for the design, rejected alternatives and open questions.

## Command-line prototype

```sh
make build                      # dist/sigstore-shell-cli (everything runs in containers)
make lint test                  # clippy, rustfmt, unit tests

sigstore-shell-cli verify app.exe                    # repo from the embedded claim (PE version string or ELF note)
sigstore-shell-cli verify app.exe --repo owner/repo  # ask about a specific repo
sigstore-shell-cli verify app.exe --rekor            # also search Rekor v1 by hash (sends the hash to a second service)
sigstore-shell-cli verify app.exe --json
```

`GH_TOKEN` or `GITHUB_TOKEN` is used for the GitHub API if set; without one the unauthenticated limit (60 requests per hour per IP) applies. The report prints the rate-limit headers.

Outcomes and exit codes:

| Outcome | Exit | Meaning |
| --- | --- | --- |
| verified | 0 | The attestation verifies against the public Sigstore root; identity is read from the certificate |
| no attestation | 10 | Providers answered and found none. Not a statement that the file is unsafe |
| mismatch | 11 | Verified, but the certificate's repo differs from the embedded claim or `--repo` |
| lookup failed | 12 | Offline, rate-limited or an error; says nothing about whether an attestation exists |
| verification failed | 13 | An attestation was returned but did not verify |
| not checked | 14 | No repo to ask about and `--rekor` not given |

Code lives in `crates/provenance-core` (PE and ELF claim reader, SHA-256, `Provider` trait with GitHub and Rekor v1 implementations, verification through [sigstore-verify](https://crates.io/crates/sigstore-verify)) and `crates/sigstore-shell-cli`.

## Explorer extension

A native 64-bit COM DLL (`crates/sigstore-shell-ext`, Rust) that adds a **Sigstore** tab to the Properties of a single selected `.exe`.

- Opening the tab reads `ProvenanceRepo` from the file and shows it as "claimed, not verified", or "no provenance information in this file". No hashing, no network.
- **Verify** hashes the file, asks GitHub for the attestation, verifies it and shows repo, owner, workflow, commit, signing time and which provider answered. It runs on a worker thread with a progress bar and a Cancel button. Pressing it is the consent to send the file's SHA-256 to `api.github.com` (and to download Sigstore's trust root).
- Results are cached by file hash in `%LOCALAPPDATA%\sigstore-shell\cache` (verified and mismatch for 7 days, "no attestation" for 1 hour; failures are never cached). A cached answer still needs the file hashed again.
- Rekor v1 search is off. To enable it, create `%LOCALAPPDATA%\sigstore-shell\settings.json` containing `{"rekor": true}`. The tab then states that the hash also goes to `rekor.sigstore.dev`, and files with no embedded repo become verifiable.
- No GitHub token is used, so the unauthenticated limit (60 requests per hour per IP) applies; the cache keeps repeat checks off the API.

### Build

```sh
make dll          # dist/sigstore_shell_ext.dll, cross-compiled to x86_64-pc-windows-gnu in the container
make wine-smoke   # registers, loads and unloads the DLL under Wine (headless)
make wine-ui      # also builds the page under a virtual display and presses Verify (needs network)
```

The DLL imports only Windows system DLLs (checked with `objdump -p`); it needs no Rust or MinGW runtime. TLS is rustls with the platform verifier, so certificates are checked against the Windows certificate store and no OpenSSL or bundled root list is involved. It is 64-bit only, so 32-bit programs that show Properties will not load it.

### Download and verify a release

Releases are built by GitHub Actions from this repo and attested with `actions/attest-build-provenance`. The DLL and the Windows CLI carry `ProvenanceRepo = non7top/sigstore-shell` in their version resource, and the Linux CLI carries it as an ELF note, so each can verify itself (`sigstore-shell-cli verify sigstore-shell-cli`). Each release has:

- `sigstore-shell-ext-<version>-windows-x64.zip`: the DLL, `register.ps1`, `unregister.ps1`, the Inno Setup script and install notes
- `sigstore_shell_ext.dll`, `sigstore-shell-cli.exe` (Windows), `sigstore-shell-cli` (Linux x86-64)
- `SHA256SUMS`
- a `.cosign.bundle` next to each file above (keyless cosign signature)

```sh
gh release download v0.1.0 --repo non7top/sigstore-shell
gh attestation verify sigstore-shell-ext-0.1.0-windows-x64.zip --repo non7top/sigstore-shell

# or with cosign
cosign verify-blob --bundle sigstore-shell-cli.cosign.bundle \
  --certificate-identity-regexp '^https://github.com/non7top/sigstore-shell/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com sigstore-shell-cli
```

Then unzip and follow `INSTALL.md` in the zip, or the steps below. Releases are cut by merging the release-please PR; commits must follow [Conventional Commits](https://www.conventionalcommits.org/).

### Install and uninstall (Windows, as administrator)

```powershell
mkdir "C:\Program Files\sigstore-shell"
copy dist\sigstore_shell_ext.dll "C:\Program Files\sigstore-shell\"
copy installer\register.ps1, installer\unregister.ps1 "C:\Program Files\sigstore-shell\"
& "C:\Program Files\sigstore-shell\register.ps1"
```

Then open the Properties of an `.exe` and look for the Sigstore tab. To remove it run `unregister.ps1`; Explorer keeps the DLL loaded until it restarts, so restart Explorer (or sign out) before deleting the file.

`register.ps1` calls `regsvr32`, which writes, under `HKLM\Software\Classes`, the CLSID with its `InprocServer32` and the handler key `exefile\shellex\PropertySheetHandlers\SigstoreShell`, and adds the CLSID to `HKLM\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved`. `installer/sigstore-shell.iss` is an Inno Setup script doing the same; it is not built here because Inno Setup runs only on Windows.

### Signing

The DLL is not Authenticode-signed. Unsigned, it loads for a user who runs `register.ps1`, but SmartScreen and some endpoint-protection products will flag it, and a real release should be signed. Sign `sigstore_shell_ext.dll` and the installer with a timestamp, for example `signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256 /a sigstore_shell_ext.dll`, before packaging. SignPath Foundation offers free signing for open-source projects. Signing is not set up in this repository yet.

### Verification status

Verified here: the unit tests (`make test`), clippy for Linux and for the Windows target, the DLL's imports and exports, and under Wine 10 the registration, `DllGetClassObject`, `IShellExtInit`/`IShellPropSheetExt::AddPages`, `DllCanUnloadNow` and unregistration. Not verified: loading in real Explorer, the Windows 11 classic Properties dialog, the page's appearance and behaviour on real Windows (Wine's result is in [project.md](project.md)), DPI scaling, and the DLL on Windows 10. Treat those as open until tried on a Windows machine.

## Publishing a project that works with it

See the promptloom setup: [non7top/promptloom](https://github.com/non7top/promptloom). You can check an attested file without this tool with `gh attestation verify <file> --repo owner/repo`.

## License

[MIT](LICENSE)
