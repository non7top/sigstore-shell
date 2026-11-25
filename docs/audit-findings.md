# sigstore-shell Audit Findings

Findings from an exploration of the sigstore-shell codebase: bugs, inconsistent behaviours, missing checks and paths that lower verification. Each finding was checked against the code (see "Checked" under each). Line numbers drift between branches, so functions are named too.

## Summary

| # | Finding | Verdict | Issue | Fix |
|---|---|---|---|---|
| 1 | `valid_repo` accepts `..` and has no length limit | confirmed, low impact | [#75](https://github.com/non7top/sigstore-shell/issues/75) | [#82](https://github.com/non7top/sigstore-shell/pull/82) |
| 2 | A failed bundle download is reported as "No attestation found" | confirmed | [#76](https://github.com/non7top/sigstore-shell/issues/76) | [#82](https://github.com/non7top/sigstore-shell/pull/82) |
| 3 | The first verified bundle ends the search | confirmed, fails in the safe direction | [#77](https://github.com/non7top/sigstore-shell/issues/77) | [#82](https://github.com/non7top/sigstore-shell/pull/82) |
| 4 | A verified result without a repo shows green and "built by the signer shown below" | confirmed, an outcome problem and not only wording | [#78](https://github.com/non7top/sigstore-shell/issues/78) | [#85](https://github.com/non7top/sigstore-shell/pull/85) |
| 5 | Mismatched attestations are cached for 7 days | overstated as written, real for a Rekor mismatch | [#84](https://github.com/non7top/sigstore-shell/issues/84) | [#85](https://github.com/non7top/sigstore-shell/pull/85) |
| 6 | A stale sidecar turns "No attestation" into "Verification failed"; only the first sidecar is tried | confirmed, partly by design | [#79](https://github.com/non7top/sigstore-shell/issues/79) | open |
| 7 | "Choose bundle..." reuses an existing sidecar | a limitation, documented but wrong | [#83](https://github.com/non7top/sigstore-shell/issues/83) | [#85](https://github.com/non7top/sigstore-shell/pull/85) |
| 8 | PE claim needs `VarFileInfo\Translation`, no size limit | confirmed from pelite's source | [#80](https://github.com/non7top/sigstore-shell/issues/80) | open (touches `pe.rs`, changed by #69 and #72) |
| 9 | Case-sensitive cache repo comparison | not reachable | none | none |
| 10 | Unbounded download and decompression in `download_bundle` | confirmed, low risk | [#81](https://github.com/non7top/sigstore-shell/issues/81) | [#82](https://github.com/non7top/sigstore-shell/pull/82) |

## 1. `valid_repo` accepts dot-only parts and any length

- File: `crates/provenance-core/src/github.rs`, `valid_repo`.
- Description: it checked only the characters `[A-Za-z0-9_.-]`, so `../target`, `foo/..` and a 100,000 character repo were "valid".
- Impact: `GithubProvider::fetch` puts the value in `https://api.github.com/repos/{repo}/attestations/...`, and URL normalisation resolves the dots: `../orgs/foo` is requested as `https://api.github.com/orgs/foo/attestations/sha256:...`. The claim comes from the file being checked, and the CLI sends `GH_TOKEN` or `GITHUB_TOKEN` with the request (the Explorer tab sends none). The request still goes to api.github.com, so it is a read-only GET to a GitHub endpoint chosen by the file's author, not a leak to another host.
- Note: `crates/sigstore-shell-ext/src/links.rs` already rejects dot-only parts and anything over 100 characters for UI links; `valid_repo` lacked both.
- Checked: a throwaway test showed `valid_repo("../x")`, `valid_repo("a/..")` and a 100k repo true, and the normalised URL above.
- Status: issue #75, fixed in PR #82.

## 2. Failed bundle downloads are reported as "No attestation found"

- File: `crates/provenance-core/src/github.rs` (`download_bundle`, `fetch`) and `crates/provenance-core/src/verify.rs` (the status chain in `verify_digest_keeping_bundle`).
- Description: `download_bundle` returned `None` on any HTTP error, timeout, dropped connection or decode failure.
- Impact: `fetch` counted that as `skipped` and left the bundle list empty; `verify` treated an empty list as `answered_empty`, which resolves to `Status::NoAttestation`. A transient failure while fetching the blob turned an existing attestation into "No attestation found". The "ignored N entries" note was added, but the verdict was wrong.
- Checked: read both functions; the new tests cover a 500 from the blob server and unusable data.
- Status: issue #76, fixed in PR #82. A blob that cannot be obtained now ends as `LookupFailed`; one that arrives unusable stays `skipped`.

## 3. The first verified bundle ends the search

- File: `crates/provenance-core/src/verify.rs`, `verify_digest_keeping_bundle`.
- Description: the loop over a provider's bundles returned as soon as one verified, as `Mismatch` when its identity was not the expected repo (an identity with no repo counts as a mismatch).
- Impact: with several valid bundles for one digest, for example Rekor v1 where anyone can sign a public hash, a foreign entry listed first hid a later one that matched the claimed repo. The result is a false `Mismatch`, never a false `Verified`, so it fails in the safe direction. The GitHub query is scoped to one repo, so mostly the Rekor, web-bundle and bundle-file paths are affected.
- Checked: read the loop; the choice is now a pure function with tests, because the repo has no two real bundles for one digest.
- Status: issue #77, fixed in PR #82.

## 4. A verified result without a repo shows green and says "built by the signer shown below"

- File: `crates/provenance-core/src/verify.rs` (`verdict_text`) and `crates/sigstore-shell-ext/src/model.rs` (`done_view`).
- Description: with no embedded claim and no `--repo`, nothing is expected, so any bundle that verifies for the file's hash is `Verified`. The tab sets the green outcome, and the headline says "Verified: this file is unchanged since it was built, and it was built by the signer shown below."
- Impact: for a bundle from `cosign sign-blob` with a personal e-mail identity there is no build provenance, only a signature, yet the text says it was built by that signer. The repo row stays on "No provenance information in this file" next to the green mark.
- Checked: read the code only; not run with an e-mail-signed bundle.
- Decision (agreed in review): this is an outcome problem, not wording. A certificate with no repo gets a neutral outcome and the headline "Signed by <signer>. This is a signature on this file, not build provenance."; a certificate that names a repo stays green even when the file has no claim, since it says which workflow built the file. The status stays `verified`.
- Status: issue #78, fixed in PR #85.

## 5. Mismatched attestations are cached for 7 days

- File: `crates/sigstore-shell-ext/src/job.rs`, `remember`.
- Description: `remember` stores both `Verified` and `Mismatch` results.
- Verdict: overstated as first written, but a narrower problem is real. A cache hit re-verifies the bundle offline and rebuilds the report against the file's current claim, so the status is recomputed each time, and the tab offers "Check online" and "Clear cache". A check made before any attestation exists ends as `NoAttestation`, which removes the entry, so it does not poison the cache. What is real: a Mismatch from a provider that is not scoped to the claimed repo (Rekor) stores a foreign identity's bundle, and a genuine attestation published later stays hidden for up to 7 days.
- Decision (agreed in review): do not cache a Mismatch unless GitHub returned it; GitHub's is scoped to the claimed repo and stable (for example after a rename), and caching it keeps unauthenticated users under the 60 requests per hour limit. Verified results from any provider stay cached. Since #82 every returned bundle is checked before a mismatch is chosen.
- Status: issue #84, fixed in PR #85.

## 6. A stale sidecar turns "No attestation" into "Verification failed"

- File: `crates/provenance-core/src/bundle_file.rs` (`beside`) and `verify.rs` (the status chain).
- Description: a `FILE.sigstore` or `FILE.cosign.bundle` beside the file is tried after the GitHub lookup. If it is for other bytes, `invalid` outranks `answered_empty`, so even when GitHub says there is no attestation the result is "ATTESTATION DID NOT VERIFY ... Do not rely on it". `beside` returns only the first sidecar that exists, so a corrupt `.sigstore` hides a valid `.cosign.bundle`.
- Verdict: the behaviour is described in `docs/details.md` for a bundle the user gave; for one only found by name it is a strong claim about a file the user never named.
- Checked: read `beside` and the status chain.
- Status: issue #79, open. Suggestion: keep `--bundle` and the picker strict; for a sidecar found by name, note that it was ignored and try every sidecar.

## 7. "Choose bundle..." reuses an existing sidecar

- File: `crates/sigstore-shell-ext/src/page.rs`, `choose_bundle`.
- Verdict: documented, and still a defect. A button that asks the user to choose a bundle must let them choose: with a stale or corrupt sidecar beside the file the user could not select a valid bundle from elsewhere. The sidecar is already tried automatically on Verify.
- Decision (agreed in review): the button always opens the file picker, starting in the file's folder.
- Status: issue #83, fixed in PR #85.

## 8. The PE claim needs `VarFileInfo\Translation`, and has no size limit

- File: `crates/provenance-core/src/pe.rs`, `read_claim`.
- Description: the string is looked up per language from `info.translation()`, which pelite 0.10.0 implements as a query of `\VarFileInfo\Translation` starting from an empty slice. A resource with a `StringFileInfo` table and no `VarFileInfo\Translation` yields no languages, so the claim is missed, against `project.md` ("any language").
- Also: the ELF reader caps the value at 256 bytes as `project.md` says; the PE reader takes any length (and `valid_repo` had no limit, see 1).
- Checked: read pelite's source; no PE without that table was built.
- Status: issue #80, open.

## 9. Case-sensitive cache lookups

- File: `crates/sigstore-shell-ext/src/cache.rs`, line `entry.repo.as_deref() == repo`.
- Verdict: not reachable. The cache exists only in the extension, is keyed by the file's hash, and the repo string comes from the same file every time, so its casing never differs. At worst a difference would cost one repeated lookup.
- Status: no issue.

## 10. Unbounded download and decompression in `download_bundle`

- File: `crates/provenance-core/src/github.rs`, `download_bundle`.
- Description: it used `resp.bytes()` and `snap::raw::Decoder::decompress_vec` with no limit on the body or the declared decompressed size, while `BundleFile` caps at 4 MiB, the cache at 1 MiB and the web-bundle download at 4 MiB.
- Impact: the blob URL comes from the GitHub API over TLS, so reaching this needs a misbehaving or compromised storage host. Low risk, but the other readers were bounded and this one was not.
- Checked: read the function; a test sends a Snappy header declaring 1 GiB and one blob over the cap. The unbounded allocation itself was not run.
- Status: issue #81, fixed in PR #82 (4 MiB body, 16 MiB declared size).
