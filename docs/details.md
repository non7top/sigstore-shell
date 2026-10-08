# Details

Command line, Explorer extension, build, install and verification status. The overview is in the [README](../README.md); the design and the embedded-claim format are in [project.md](../project.md).

## Command line

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

A native 64-bit COM DLL (`crates/sigstore-shell-ext`, Rust) that adds a **Provenance** tab to the Properties of a single selected `.exe`.

- Opening the tab reads `ProvenanceRepo` from the file and shows it as "claimed, not verified", or "no provenance information in this file". No hashing, no network.
- **Verify** hashes the file, asks GitHub for the attestation and verifies it. It runs on a worker thread with a progress bar and a Cancel button. Pressing it is the consent to send the file's SHA-256 to `api.github.com` (and to download Sigstore's trust root). After a result the consent sentence goes away and the button reads **Verify again**.
- The tab opens with one muted line explaining the word "provenance" (where the file came from: which repo and which workflow published it), then the claimed repository in bold with **Verify** at the top right. The consent sentence sits under it until a result exists.
- The button shows GitHub's request count (`Verify  52/60`) only after a lookup in that tab, taken from that response's headers. It is never stored, so a freshly opened tab says plain `Verify`; a cached answer leaves it unchanged. The count is per IP and shared with other tools; the button's tooltip says so. Under the result a muted line gives the reset time, or says the answer came from the cache and when.
- After a result the repository shown is the one in the certificate (bold, red if it differs from the file's claim, with the claim named below it), with the outcome icon (green tick, red cross, grey dash for "no attestation", which does not mean unsafe, grey warning for a failed lookup) and the verdict. When GitHub's limit is nearly used up or used up, the verdict says so. The details box (owner, workflow, full commit, ref, signing time, signer, who answered, SHA-256) is monospace and wrapped at path separators, so it needs no sideways scrolling. With high contrast on, the icon is replaced by a text symbol in the system text colour.
- Links (commit, workflow file at that commit, build run) open only when clicked, and only for a verified result. Each URL is rebuilt from a validated `owner/repo`, a hex commit and a workflow path directly under `.github/workflows/`; a build-run URL is accepted only if it is this repo's `actions/runs/<id>` page. Anything else gets no link. **Copy SHA-256** and **Copy signer** put the full values on the clipboard.
- The bottom line says what the check rests on, with links to sigstore.dev and to this project. Icon sources and licences are in [resources/icons/README.md](../resources/icons/README.md).
- Results are cached by file hash in `%LOCALAPPDATA%\sigstore-shell\cache` (verified and mismatch for 7 days, "no attestation" for 1 hour; failures are never cached). A cached answer still needs the file hashed again.
- Rekor v1 search is off. To enable it, create `%LOCALAPPDATA%\sigstore-shell\settings.json` containing `{"rekor": true}`. The tab then states that the hash also goes to `rekor.sigstore.dev`, and files with no embedded repo become verifiable.
- No GitHub token is used, so the unauthenticated limit (60 requests per hour per IP) applies; the cache keeps repeat checks off the API. A 5xx answer from GitHub is retried once after about a second (noted in the details); 4xx and rate limiting are not retried.

### Build

```sh
make dll          # dist/sigstore_shell_ext.dll, cross-compiled to x86_64-pc-windows-gnu in the container
make wine-smoke   # registers, loads and unloads the DLL under Wine (headless)
make wine-ui      # also builds the page under a virtual display and presses Verify (needs network)
```

The DLL imports only Windows system DLLs (checked with `objdump -p`); it needs no Rust or MinGW runtime. TLS is rustls with the platform verifier, so certificates are checked against the Windows certificate store and no OpenSSL or bundled root list is involved. It is 64-bit only, so 32-bit programs that show Properties will not load it.

### Download and verify a release

Releases are built by GitHub Actions from this repo and attested with `actions/attest-build-provenance`. The installer, the DLL and the Windows CLI carry `ProvenanceRepo = non7top/sigstore-shell` in their version resource, and the Linux CLI carries it as an ELF note, so each can verify itself (`sigstore-shell-cli verify sigstore-shell-cli`). Each release has:

- `sigstore-shell-ext-<version>-windows-x64.zip`: the DLL, `register.ps1`, `unregister.ps1`, and install notes
- `sigstore-shell-Setup-<version>.exe`: installer (admin, Add/Remove Programs entry, registers the tab)
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

Then open the Properties of an `.exe` and look for the Provenance tab. To remove it run `unregister.ps1`; Explorer keeps the DLL loaded until it restarts, so restart Explorer (or sign out) before deleting the file.

`register.ps1` calls `regsvr32`, which writes, under `HKLM\Software\Classes`, the CLSID with its `InprocServer32` and the handler key `exefile\shellex\PropertySheetHandlers\SigstoreShell`, and adds the CLSID to `HKLM\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved`. `installer/sigstore-shell.nsi` writes the same keys with NSIS registry instructions, so the installer never has to load the DLL; a unit test in `registration.rs` checks the script still names the same CLSID and handler key. `make installer` builds it with plain `makensis` on Linux, no Wine, because the uninstaller is only written, not run. Each release also ships `sigstore-shell-Setup-<version>.exe`, carrying the same `ProvenanceRepo` claim as the DLL.

### Signing

The DLL is not Authenticode-signed. Unsigned, it loads for a user who runs `register.ps1`, but SmartScreen and some endpoint-protection products will flag it, and a real release should be signed. Sign `sigstore_shell_ext.dll` and the installer with a timestamp, for example `signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256 /a sigstore_shell_ext.dll`, before packaging. SignPath Foundation offers free signing for open-source projects. Signing is not set up in this repository yet.

### Verification status

Verified here: the unit tests (`make test`), clippy for Linux and for the Windows target, the DLL's imports and exports, and under Wine 10 the registration, `DllGetClassObject`, `IShellExtInit`/`IShellPropSheetExt::AddPages`, `DllCanUnloadNow` and unregistration. Not verified: loading in real Explorer, the Windows 11 classic Properties dialog, the page's appearance and behaviour on real Windows (Wine's result is in [project.md](../project.md)), the icons and layout, DPI scaling, and the DLL on Windows 10. Treat those as open until tried on a Windows machine.
