# Rekor v2 options

Notes for issue #2: the optional Rekor provider searches Rekor v1 by hash, and Rekor v2 has no such search. What follows is what the sources say, what each option would need, and what has not been tried. Nothing here is built.

## What the sources say

- Rekor v2 removed the search index: "The search index has been removed, since this was a best-effort service." A replacement is planned as "a dedicated service backed by a verifiable index." There are no Get-By-Log-Index or Get-By-Leaf-Hash APIs. Clients that want an inclusion proof fetch tiles themselves ([Rekor v2 GA](https://blog.sigstore.dev/rekor-v2-ga/)).
- The same post discourages reading proofs back from the log and says the inclusion proof returned at upload "should be persisted alongside artifacts and their signatures". That is the case for shipping the bundle with the file instead of searching for it later.
- Authenticode hashes a PE without the checksum, the certificate table entry in the header and the certificate table itself ([Understanding PE signatures](https://learn.microsoft.com/en-us/windows/win32/secbp/understanding-pe-signatures)). Windows verification rejects a signature when something follows the attribute certificate table, and MS13-098 added a check for non-zero data after the PKCS #7 structure inside the table. In the 2013 post that stricter check was opt-in ("will not be enabled by default today"), with a stated plan to treat such binaries as unsigned later ([MSRC, 10 Dec 2013](https://www.microsoft.com/en-us/msrc/blog/2013/12/ms13-098-update-to-enhance-the-security-of-authenticode)). What current Windows does by default is not established by these sources.

## Options

1. **Sidecar or linked bundle (already how this tool works with v2 signers).** A cosign bundle made at signing time carries the transparency log entry and its proof, so verification is offline and needs no search. `--bundle`, the bundle beside the file, the embedded `ProvenanceBundle` link and the opt-in web bundle (#63, #69, #72) deliver it. This does not depend on the log's search at all. Not tested: that `sigstore-verify` 0.16 accepts a bundle whose entry comes from Rekor v2. The test bundles in this repo have entries of kind version `0.0.1` and a signed entry timestamp (`inclusionPromise`), which the GA post says v2 no longer returns, so I take them to be v1 entries.
2. **Trailer appended to the signed bytes** (the idea in the issue): the verifier strips it, hashes the rest and verifies offline. For an unsigned PE or an ELF this is plausible, since installers such as NSIS already append data to a PE. It does not combine with an Authenticode signature: data after the certificate table breaks the signature on Windows, and data inside the table (the unauthenticated attributes of the PKCS #7 blob, where timestamps live) means the stripped file is not the file that was hashed. This repository does not Authenticode-sign today (docs/details.md, "Signing"), so a trailer would close the door on adding it later unless the order is designed first. Also open: how Defender and SmartScreen treat an overlay on an unsigned PE (not tested), and whether `strip` or packers keep an ELF trailer (not tested).
3. **Wait for the planned search service.** Cheapest for us: a new provider next to `rekor-v1`, off by default like it. The post gives no date or API, so there is nothing to build against yet.
4. **Fetch tiles ourselves.** Possible in principle (the clients do), but it needs the log index, which is the thing we do not have for a bare file. It only helps when the index is carried with the file, which is option 1 or 2 again.

## Suggestion

Keep option 1 as the answer for signers on v2: it needs no format change and already exists. Revisit when the search service has an API (option 3). Do not build the trailer (option 2) before deciding whether Authenticode signing will ever be wanted, because the two exclude each other as described above.

## Not checked

Nothing in this note was run: no Rekor v2 bundle was produced or verified, and no Windows behaviour was observed. The Authenticode statements come from Microsoft's documentation and its 2013 bulletin only.
