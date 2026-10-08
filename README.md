# sigstore-shell

**Problem.** A signature on a Windows release names whoever holds the certificate, a company or a person. That tells you little today: a name says nothing about how the file was built or from which source code, and that was never what a signature covered. Meanwhile the usual way to get software in 2026 is to download a binary from GitHub and run it, with the open repo as the only verification on offer, and detached GPG signatures, once common for this, have mostly gone.

Sigstore provenance fills that gap: a signed, public record of which repo, workflow and commit built this exact file, which anyone can check against the source. As Let's Encrypt did for certificates, free and automatic issuance can change what people expect from a release. Provenance does not say the code is safe, only where the file came from.

This project shows that record where you look at the file: the Properties dialog in Explorer, or a command.

## What it does

- Adds a **Sigstore** tab to an exe's Properties in Explorer: repo, owner, workflow, commit and signing date, verified from the file's Sigstore attestation.
- Ships the same check as a command line (`sigstore-shell-cli verify <file>`, Windows and Linux).

Nothing is hashed or sent anywhere until you press **Verify** (or run the CLI). That click is the consent to send the file's SHA-256 to GitHub.

## How it works

1. The publishing project embeds `owner/repo` in the file and attests it with `actions/attest-build-provenance`. Format: [Embedded claim](project.md#embedded-claim).
2. The tool shows that string as an unverified claim, then hashes the file, fetches the attestation for that digest from GitHub and verifies it against Sigstore's roots.
3. The identity shown comes only from the certificate, never from the embedded string; a mismatch is flagged. "No attestation" means none was found, not that the file is unsafe.

Rekor v1 (search by hash, for files with no embedded repo) is optional and off by default.

## Use

```sh
gh release download --repo non7top/sigstore-shell
sigstore-shell-cli verify app.exe
```

Install the Explorer tab on Windows by running `register.ps1` from the release zip as administrator (`INSTALL.md` is in the zip). Releases are attested and cosign-signed; check one with `gh attestation verify <file> --repo non7top/sigstore-shell`.

## Status

Early development. The CLI is tested against real attested releases. The Explorer tab is built and smoke-tested under Wine but not yet run in real Explorer, and the DLL is not Authenticode-signed. Details: [docs/details.md](docs/details.md); design: [project.md](project.md).

## Publish a project that works with it

Embed the claim and attest the files, as [non7top/promptloom](https://github.com/non7top/promptloom) does.

## License

[MIT](LICENSE)
