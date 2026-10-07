# sigstore-shell

A Windows Explorer extension that adds a **Sigstore** tab to a file's Properties dialog. It shows which GitHub repo built an exe, with the workflow, commit and signing date, verified from the file's Sigstore attestation.

Status: early development. The command-line prototype comes first; the Explorer extension follows.

## How it works

1. The publishing project writes `ProvenanceRepo = owner/repo` into the exe's version resource and attests the exe with `actions/attest-build-provenance`.
2. The tab shows that string as an unverified claim. Pressing **Verify** hashes the file, fetches the attestation for that digest from GitHub, checks it against Sigstore's roots and shows the identity from the certificate.

The embedded string is only a hint. The identity shown always comes from the certificate, and a mismatch is flagged. Nothing is hashed or sent over the network until you press Verify.

Optionally, Rekor v1 can be searched by hash for files with no embedded repo. It is off by default because it sends the hash to a second service.

See [project.md](project.md) for the design, rejected alternatives and open questions.

## Publishing a project that works with it

See the promptloom setup: [non7top/promptloom](https://github.com/non7top/promptloom). You can check an attested file without this tool with `gh attestation verify <file> --repo owner/repo`.

## License

[MIT](LICENSE)
