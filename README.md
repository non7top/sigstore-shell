# sigstore-shell

A Windows Explorer extension that adds a **Sigstore** tab to a file's Properties dialog. It shows which GitHub repo built an exe, with the workflow, commit and signing date, verified from the file's Sigstore attestation.

Status: early development. The command-line prototype (below) works; the Explorer extension follows.

## How it works

1. The publishing project writes `ProvenanceRepo = owner/repo` into the exe's version resource and attests the exe with `actions/attest-build-provenance`.
2. The tab shows that string as an unverified claim. Pressing **Verify** hashes the file, fetches the attestation for that digest from GitHub, checks it against Sigstore's roots and shows the identity from the certificate.

The embedded string is only a hint. The identity shown always comes from the certificate, and a mismatch is flagged. Nothing is hashed or sent over the network until you press Verify.

Optionally, Rekor v1 can be searched by hash for files with no embedded repo. It is off by default because it sends the hash to a second service.

See [project.md](project.md) for the design, rejected alternatives and open questions.

## Command-line prototype

```sh
make build                      # dist/sigstore-shell-cli (everything runs in containers)
make lint test                  # clippy, rustfmt, unit tests

sigstore-shell-cli verify app.exe                    # repo from the ProvenanceRepo version string
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

Code lives in `crates/provenance-core` (PE version-resource reader, SHA-256, `Provider` trait with GitHub and Rekor v1 implementations, verification through [sigstore-verify](https://crates.io/crates/sigstore-verify)) and `crates/sigstore-shell-cli`.

## Publishing a project that works with it

See the promptloom setup: [non7top/promptloom](https://github.com/non7top/promptloom). You can check an attested file without this tool with `gh attestation verify <file> --repo owner/repo`.

## License

[MIT](LICENSE)
