# sigstore-shell

A Windows Explorer extension that adds a **Sigstore** tab to a file's Properties dialog. The tab shows where an exe came from: GitHub repo, owner, workflow, commit and signing date. Promptloom is the first project to publish files that this extension can read.

Status: early implementation. Items marked **unverified** come from documentation or memory and need a real check first.

## Goal

Tell where an exe came from by looking only at the file, with a lookup keyed by the file's hash. This is the same model `cosign verify` uses for container images: the signature is not inside the artifact, it is found by digest.

## Why not the obvious approaches

- **Cosign bundles next to the file.** A `.cosign.bundle` is a detached file. Explorer ignores it, and a downloaded exe does not come with it.
- **Embedding the signature in the exe.** The signature covers the exe's hash, so adding bytes to the exe breaks it. `cosign verify-blob` can't handle a custom trailer format.
- **Searching the public Sigstore log (Rekor) by hash.** Rekor v1 supports it (`/api/v1/index/retrieve`) and is in maintenance mode but stays the public default "for the foreseeable future". Rekor v2 removed search by hash, and the replacement is only a planned separate service. A design that depends on it will break as signers move to v2.
- **Authenticode.** SmartScreen only trusts Authenticode. Sigstore's roots are not in the Windows trust store, and Fulcio certificates last about 20 minutes, so Sigstore can't replace Authenticode. They do different jobs: Authenticode makes Windows accept the file, Sigstore says which repo built it. (SignPath Foundation offers free Authenticode signing for open-source projects; that is separate work in each publishing project.)

## Mechanism

1. **At build time**, the publishing project writes its repo into the file as an embedded claim (see [Embedded claim](#embedded-claim)): a version-resource string in a PE, a note in an ELF. This happens before the file is hashed or attested.
2. **At release time**, the project attests each exe with `actions/attest-build-provenance`. GitHub stores a Sigstore-signed provenance record keyed by the file's SHA-256 digest. Both the app exe and the installer need their own attestation; the inner exe has a different hash from the installer.
3. **In Explorer**, the tab opens instantly and does no network or hashing. It reads `ProvenanceRepo` from the file (no network needed) and shows it labelled as an unverified claim. Pressing **Verify** then:
   1. hashes the file;
   2. asks GitHub for that repo's attestations for that digest;
   3. verifies the attestation's certificate chain against Sigstore's roots;
   4. shows repo, owner, workflow, commit and signing time from the certificate.

### Lookup providers

Lookup is a swappable provider; the verifier that checks the certificate chain and reads the identity is shared.

- **GitHub attestations (default):** used when the file has `ProvenanceRepo`.
- **Rekor v1 (optional, off by default):** searches the public log by hash, so it can find a signer for a file with no embedded repo. It works only while signers use Rekor v1, since v2 dropped search by hash. Enabling it is a separate opt-in because it sends the file's hash to a second service.
- A Rekor hit proves that someone signed this hash, not that the signer is one the user expects, so the tab must show the signer prominently and let the user judge.
- The tab shows which provider answered. The identity always comes from the certificate, never from the provider's response alone.

### Trust rule

The embedded repo string is a hint only. Anyone can write any repo into an exe. The extension must show the identity from the attestation's certificate, never from the embedded string, and must flag a mismatch between the two.

- A false repo in the string finds no attestation, so the tab says "no provenance".
- A malicious file that points at its own repo and is really attested there shows that repo truthfully. The user sees who built it and decides.

## Embedded claim

The claim says which repo the publisher says built the file. This section is normative; the constants are in `crates/provenance-core/src/claim_format.rs`, which the build scripts share with the reader.

- **Value:** `owner/repo`, matching `^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$` (exactly one `/`, both parts non-empty), UTF-8 without a terminating NUL. At most 256 bytes.
- **Hint only:** the claim is never evidence. The identity always comes from the verified certificate; a claim that differs from it is a mismatch.
- **Carrier PE:** a string named `ProvenanceRepo` in the `VS_VERSIONINFO` string table (any language). Written by the build from the `PROVENANCE_REPO` environment variable when it is set.
- **Carrier ELF:** an `SHT_NOTE` section named `.note.provenance` (flags `SHF_ALLOC`, alignment 4) holding one note in the ELF note layout: `namesz`, `descsz` and `type` as 32-bit words in the file's byte order, then the name `ProvenanceRepo` plus a NUL (`namesz` = 15) padded with zeros to a multiple of 4, then the `desc`, then zero padding to a multiple of 4. Note `type` is `1`. `desc` is the value above, `descsz` is its exact length without NUL or padding. Written by the CLI's `build.rs` when `PROVENANCE_REPO` is set and the target is Linux; it survives `--gc-sections` and `strip`.
- **No claim:** with `PROVENANCE_REPO` unset (local and PR builds) nothing is embedded.
- **Readers** pick the carrier by magic: `MZ` is PE, `\x7fELF` is ELF, anything else has no claim. A reader must never panic or read out of bounds on hostile input. A PE or ELF whose headers cannot be parsed is an error (shown as unreadable, not as "no claim"). A parseable file without the claim, a note of another name or type, a `desc` longer than 256 bytes, a `desc` that is not UTF-8, or an empty value yields no claim. ELF readers scan every `SHT_NOTE` section, not only by section name, and use the first note that yields a value. Consumers must still validate the value as `owner/repo` before using it (the CLI and the extension do, and ignore an invalid one with a note).

## Tab behaviour

- Tab label: "Sigstore".
- **Verification is an explicit button, not automatic.** Hashing a large exe, the network call and the signature check can take significant time, and the click is also the consent to send the file's hash to GitHub. Opening the tab never does any of that.
- Before the button is pressed: the embedded repo, if any, labelled "claimed, not verified", or "no provenance information in this file".
- After: verified (repo/owner/workflow/commit/date), no attestation found, mismatch with the embedded claim, or lookup failed (offline, rate-limited). Run it off the UI thread with a progress indicator and a cancel, and cache the result per file hash.
- "No attestation" means none was found, not "unsafe". Most exes in the wild have none.

## Implementation notes

- A shell property-sheet extension is a native COM DLL (`IShellPropSheetExt`). **Unverified:** Microsoft does not support .NET in-process shell extensions, so C++ or Rust. It needs an installer with admin rights and registration per file type, and the DLL must itself be Authenticode-signed.
- Verification should use an existing Sigstore library (e.g. sigstore-rs), not custom crypto.
- A right-click "Verify provenance" entry is much cheaper than a tab if the tab proves too heavy.
- Windows 11's classic Properties dialog support for extensions: **unverified**.

## Open questions

1. Can the GitHub attestations API be queried without a token for a public repo, and what are the rate limits? The prototype answers this. If it can't, the Rekor v1 provider becomes the default. Private-repo attestations use GitHub's own trust root, not the public Sigstore instance.

   **Answer (prototype run 2026-10-07, `sigstore-shell-cli` against `cli/cli` v2.102.0 `gh_2.102.0_windows_amd64.zip`, no token):**
   - Yes, unauthenticated works for a public repo. `GET /repos/cli/cli/attestations/sha256:<digest>` returned 200 with two attestations, and the CLI verified one against the public Sigstore root (TUF-fetched) and read repo, owner, workflow, ref, commit and signing time from the certificate. `gh attestation verify ... --repo cli/cli` on the same file also exits 0.
   - Rate limit: `x-ratelimit-limit: 60`, `x-ratelimit-resource: core`, i.e. the normal unauthenticated 60 requests per hour per IP, shared with everything else on that IP. Each lookup costs one request (a 404 or an empty list costs the same). The 403/429 handling for an exhausted limit is written but was not exercised, so its real response is unverified. A token (`GH_TOKEN`/`GITHUB_TOKEN`) is supported and raises the limit; not tested here.
   - Surprise 1: the response no longer inlines the bundle. `bundle` is `null` and `bundle_url` is a short-lived signed Azure blob URL to Snappy-compressed bundle JSON. The prototype downloads it without sending the GitHub token. The responses also carry `Deprecation: 2026-03-10` and `Sunset: 2028-03-10` headers (see the note below on what they mean).
   - Surprise 2: one of the two attestations (`initiator: github`) has no transparency-log entry and fails verification against the public root ("must have an inclusion proof"). I assume it is a GitHub-signed release attestation using GitHub's own trust root (`gh` showed only the other one); not confirmed. The CLI verifies each bundle and reports the rest as a note.
   - Only the zip is attested; `gh.exe` extracted from it has no attestation (200 with an empty list), which matches the plan that each shipped file needs its own attestation.
   - The Rekor v1 provider also works: for `sigstore/cosign` v3.1.3 `cosign-linux-arm64`, `--rekor` found a `hashedrekord` entry by hash and verified it (signer `keyless@projectsigstore.iam.gserviceaccount.com`, issuer accounts.google.com). Only `hashedrekord` v0.0.1 entries are turned into bundles; DSSE entries are not found by artifact hash.
   - Conclusion: GitHub attestations stay the default provider; Rekor is not needed as the default. Pagination (`per_page=100`, no paging) and private-repo (GitHub trust root) attestations are not handled yet.

   **Deprecation/Sunset headers (checked 2026-10-07):** they announce the end of support for REST API version `2022-11-28` (deprecation 2026-03-10, sunset 2028-03-10); they do not announce a replacement of the attestations endpoint. `GET https://api.github.com/versions` returns `2026-03-10` and `2022-11-28`. GitHub's versioning docs say requests naming a closed-down version get `410 Gone`, and requests with no version header fall back to the oldest supported version. The breaking-changes page for `2026-03-10` says the `bundle` field is removed from the repo, org and user attestation list responses and `bundle_url` replaces it. `provenance-core` already uses an inline `bundle` if present and otherwise downloads `bundle_url`, so no code change was needed. With `curl`, sending `X-GitHub-Api-Version: 2026-03-10` still returned `x-github-api-version-selected: 2022-11-28` and the same body shape (both `bundle` and `bundle_url` keys, `bundle` null), so the version header stays at `2022-11-28`; revisit before 2028-03-10.

## Decisions

- **Name:** keep `sigstore-shell`. If Sigstore asks, rename, and first ask them to take the project into their organization instead.
- **Version resource key:** setting a custom key (electron-builder or an rcedit step) is the publishing project's concern, not this one's.
- **Signing order:** the publishing project's concern.
- **Providers:** GitHub attestations and Rekor v1 now. Rekor v2 has no search, so options such as a bundle trailer appended after signing are tracked in an issue.

## Plan

1. **Prototype (command line, done; see open question 1):** read `ProvenanceRepo` from an exe, hash it, query GitHub, verify, print the result. Test against a real attested promptloom release. This answers question 1.
2. **Promptloom changes** (tracked in non7top/promptloom#151): embed the repo in the app exe and installer, attest both, document `gh attestation verify <file> --repo non7top/promptloom` in the README.
3. **Shell extension (built, see README):** COM property-sheet DLL in `crates/sigstore-shell-ext`, with an on-disk cache and Rekor opt-in through `settings.json`. Exercised under Wine only; not yet run in real Explorer.
4. **Installer and signing** for the extension itself.

## First user

[non7top/promptloom](https://github.com/non7top/promptloom): see issue #151.
