# sigstore-shell

**Problem.** These days `org/repo` is your guide. Most of the software on a new workstation is open source; you find it by searching, the search lands on a GitHub repo, and you download a binary from its releases: Go tools on Linux, media players and utilities on Windows. What vouches for that binary? On Windows, a signature shows you a name. For a media player you've never heard of, you can't tell who that is, what the name proves, or how it ties to the file in front of you. On Linux, GPG signatures exist and people sometimes check them. Neither kind of signature says how the file was built or from which source code.

But the source is public. Sigstore provenance ties the file to it: a signed, public record of which repo, workflow and commit published this exact file. The trust moves from a name to a public repo you can read. The tool shows which repo published the file; compare it with the repo you downloaded from. No signature says the code is safe, and this doesn't either. It says which workflow, at which commit, produced the file; read that workflow to see how.

This project shows that record where you look at the file: the Properties dialog in Explorer, or a command.

## What it does

- Adds a **Sigstore** tab to an exe's Properties in Explorer: repo, owner, workflow, commit and signing date, verified from the file's Sigstore attestation.
- Ships the same check as a command line (`sigstore-shell-cli verify <file>`, Windows and Linux).

Nothing is hashed or sent anywhere until you press **Verify** or run the CLI. Either one sends the file's SHA-256 to GitHub.

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

Install the Explorer tab on Windows by running `register.ps1` from the release zip as administrator (`INSTALL.md` is in the zip). Releases are attested and cosign-signed (from the first one on); check a file with `gh attestation verify <file> --repo non7top/sigstore-shell`.

## Status

Early development. No release is published yet. The CLI is tested against real attested releases. The Explorer tab is built and smoke-tested under Wine but not yet run in real Explorer, and the DLL is not Authenticode-signed. Details: [docs/details.md](docs/details.md); design: [project.md](project.md).

## Publish a project that works with it

Embed the claim and attest the files, as [non7top/promptloom](https://github.com/non7top/promptloom) does.

## License

[MIT](LICENSE)
