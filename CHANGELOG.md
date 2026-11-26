# Changelog

## [0.4.1](https://github.com/non7top/sigstore-shell/compare/v0.4.0...v0.4.1) (2026-11-26)


### Bug Fixes

* **build:** show the release version as File version in Explorer ([4df747d](https://github.com/non7top/sigstore-shell/commit/4df747dc750579d26cc376aaa0648aa68c9f5d2e))
* **cli:** exit 15 when a bundle verifies but no repo was expected ([d62cf34](https://github.com/non7top/sigstore-shell/commit/d62cf3482f4e6ca3b44e0cb8d632431583256b5c))
* **rekor:** read at most 25 entries and report one that fails instead of aborting the search ([c4c4143](https://github.com/non7top/sigstore-shell/commit/c4c41437e86426f5a28c42da5e7c49cd76e51fee))
* **verify:** close the gaps found in the app-code audit ([fbc4b3b](https://github.com/non7top/sigstore-shell/commit/fbc4b3bee3e1c5f8c3a318b2cb24567d2f7d9d84))
* **verify:** count a DSSE attestation as provenance only when its statement is SLSA build provenance ([522909d](https://github.com/non7top/sigstore-shell/commit/522909d2ee58f7ce2c3ffafcd78914056ad079ef))
* **verify:** say in the result when the built-in trust root was used instead of the TUF one ([b621766](https://github.com/non7top/sigstore-shell/commit/b6217666b2f782bbc919856d49f24b74e9b7c6e0))
* **web-bundle:** never connect to loopback, private or link-local addresses for a link read from a file ([25e3fce](https://github.com/non7top/sigstore-shell/commit/25e3fce8c5bddb9ae81785612ef51d8626de1643))


### Dependencies

* bump the cargo group with 2 updates ([9a07012](https://github.com/non7top/sigstore-shell/commit/9a07012cd1833511a4e427cd8c35adff269f7a45))

## [0.4.0](https://github.com/non7top/sigstore-shell/compare/v0.3.0...v0.4.0) (2026-11-25)


### Features

* **build:** embed the link to each release asset's cosign bundle and check it in the release ([d252ec8](https://github.com/non7top/sigstore-shell/commit/d252ec8eac7fc730414372c314d714229be098dc))
* **build:** put the repo into CompanyName and Publisher in release builds ([478f4a4](https://github.com/non7top/sigstore-shell/commit/478f4a4d8bc9018edbe0952ce80b0d53a0bbb12f))
* **build:** write the repo into CompanyName and the installer Publisher in release builds ([6ec9672](https://github.com/non7top/sigstore-shell/commit/6ec9672f091a0d50dbf4e3ccb6801b7753bf4a1a))
* **cli:** add --web-bundle to download the bundle the file links to ([8136d59](https://github.com/non7top/sigstore-shell/commit/8136d590ecd1a8d6e00f82a64d67f444ce498e81))
* **cli:** verify a bundle downloaded from an https URL ([2c2b59c](https://github.com/non7top/sigstore-shell/commit/2c2b59cf0be571901b25f94846e316d40b419ad9))
* **cli:** verify a bundle downloaded from an https URL with --bundle-url ([d0b171c](https://github.com/non7top/sigstore-shell/commit/d0b171c837c579d89a91510eb182c38418218d36))
* **core:** guess the release bundle URL from the repo claim, ProductVersion and file name ([4898e90](https://github.com/non7top/sigstore-shell/commit/4898e909c529a58db359a71a5bb8b826933c674b))
* **core:** read an optional bundle link from the embedded claim in PE and ELF files ([ba47d61](https://github.com/non7top/sigstore-shell/commit/ba47d61fc6d1a7931a49f6c08d35d3505b52982e))
* **core:** read where a download came from out of its Zone.Identifier stream ([a182538](https://github.com/non7top/sigstore-shell/commit/a182538e8e8d613334c3185409857249421f8d79))
* embed a bundle link in the claim and use it from the tab and the CLI ([719af3e](https://github.com/non7top/sigstore-shell/commit/719af3e7691e09fc669a6aa4c73d735b07fa66be))
* **ext:** add the web_bundle setting that downloads the bundle the file links to ([42ee644](https://github.com/non7top/sigstore-shell/commit/42ee6440e1d14a32cfae072866b0c79bb87d5368))
* **ext:** show the Zone.Identifier download hint as an unverified detail row ([ed5a182](https://github.com/non7top/sigstore-shell/commit/ed5a182c3a0efbf64a66db59a8827e48e448c07b))
* guess the release bundle link for files that do not embed one ([28017c4](https://github.com/non7top/sigstore-shell/commit/28017c479877272168a556a19a4aa73eda555e4e))
* show the Zone.Identifier download hint as an unverified detail ([28365c5](https://github.com/non7top/sigstore-shell/commit/28365c59297764fc14c710fcdb7664ca7608b894))
* use the guessed bundle link for --web-bundle and the web_bundle setting ([dc86308](https://github.com/non7top/sigstore-shell/commit/dc86308d94e1f7907d435016f9761cb84e2d10e0))


### Bug Fixes

* **core:** check every bundle a provider returns before choosing the one to report ([a0c04fb](https://github.com/non7top/sigstore-shell/commit/a0c04fb7db9c7d0e65b0623343cdea6ef328a604))
* **core:** find the PE claim without a Translation table and ignore values over the size limits ([7d64a9a](https://github.com/non7top/sigstore-shell/commit/7d64a9aed243519fe04b218c14c5f713a82448b9))
* **core:** harden repo validation, bundle selection and the bundle download ([8ff3508](https://github.com/non7top/sigstore-shell/commit/8ff35087bf551fd87c490bb1e1922eec2f726270))
* **core:** ignore a bundle found beside the file when it is not for it, and try every sidecar ([db811c5](https://github.com/non7top/sigstore-shell/commit/db811c5bf5cf86cd30a7109b37292e0a7bd759c3))
* **core:** read the PE claim without a Translation table and ignore a stale sidecar ([1a26a83](https://github.com/non7top/sigstore-shell/commit/1a26a833d92e4d7c153a92403b96a37d46b44e6c))
* **core:** reject dot-only and over-long parts in valid_repo ([affe3f5](https://github.com/non7top/sigstore-shell/commit/affe3f5886d7980a6da641abe4414c1edd1816c7))
* **core:** report a bundle that could not be downloaded as a failed lookup and bound the download ([2564688](https://github.com/non7top/sigstore-shell/commit/25646883c298fa4430575e23a13e726109c02040))
* do not describe a signature without a repo as a build, and show it as neutral ([5282d22](https://github.com/non7top/sigstore-shell/commit/5282d2252f352f4ef0032fa1f2326e09b91776ac))
* **ext:** adapt to jiff 1.0 ([7fcefe9](https://github.com/non7top/sigstore-shell/commit/7fcefe9eafdbc30ee1c0e49d0be64ef57281a71a))
* **ext:** always open the file picker from the bundle button ([7b467f3](https://github.com/non7top/sigstore-shell/commit/7b467f39e4c32ab98e165b48aa0d6fc852425b08))
* **ext:** cache a mismatch only when GitHub returned it ([dbaa87d](https://github.com/non7top/sigstore-shell/commit/dbaa87d856aeb6c7648af5cffb8aaf957988539d))
* **ext:** call jiff 1.0's to_zoned_datetime, which replaces to_zoned ([8ad4660](https://github.com/non7top/sigstore-shell/commit/8ad46604930b69d37b76ec34da0e7b2ebf6b640a))
* neutral result for a signature without a repo, picker always opens, cache only GitHub mismatches ([36b56c5](https://github.com/non7top/sigstore-shell/commit/36b56c5bad2e2e706cea58f2a1e8512587f253ba))
* say that the web bundle is downloaded only if the lookups before it do not verify the file ([77a159c](https://github.com/non7top/sigstore-shell/commit/77a159c46be6d3edfd4d5e146a564c994cbfbaf4))


### Dependencies

* bump the cargo group with 2 updates ([d3b2307](https://github.com/non7top/sigstore-shell/commit/d3b2307e88d0c8baa1ef9065695d61ffa0987085))

## [0.3.0](https://github.com/non7top/sigstore-shell/compare/v0.2.0...v0.3.0) (2026-11-25)


### Features

* **build:** report the release version and commit in every binary ([d318336](https://github.com/non7top/sigstore-shell/commit/d318336f62abc6d3698a4085d8df95129bbd6845))
* **ext:** Bundle file button uses the bundle beside the file, else opens a file picker ([c3347e6](https://github.com/non7top/sigstore-shell/commit/c3347e6a25e29b7bb21b37ec657f02d398240933))
* **ext:** Bundle file button uses the bundle beside the file, else opens a file picker ([65439a1](https://github.com/non7top/sigstore-shell/commit/65439a18b06997bf31aae24dfef40884400df67f))
* result wording states unchanged and built-by, and what no attestation cannot tell ([5f4448e](https://github.com/non7top/sigstore-shell/commit/5f4448e5bf83a0277067f0b592239d3241fa5f48))
* result wording states unchanged and built-by, and what no attestation cannot tell ([f9758b6](https://github.com/non7top/sigstore-shell/commit/f9758b60736ab8791f886b07ee6f99f28cab4f8a))
* **scoop:** add a Scoop bucket, a reusable Scoop workflow and build version and commit in every binary ([db27905](https://github.com/non7top/sigstore-shell/commit/db279055671c020f55fe60122d0578dd961dfb1b))
* **scoop:** add a Scoop manifest for sigstore-shell-cli ([9d1163d](https://github.com/non7top/sigstore-shell/commit/9d1163dbd7c76f6495c460ab1bc07eb0aa5afd7b))
* verify against a bundle file, explicit with --bundle or beside the file ([ae6687b](https://github.com/non7top/sigstore-shell/commit/ae6687bfb0ceb2525bfa726881bcd645fc3fce58))


### Bug Fixes

* **ext:** keep the consent text as it was, its longer form left stale pixels under the links ([0d4fcb3](https://github.com/non7top/sigstore-shell/commit/0d4fcb3dfa1ed0282dbd1878b69cb3c194c9db27))
* keep the checkpoint dash in the bundle fixture JSON-escaped so hooks leave it alone ([8dc34bd](https://github.com/non7top/sigstore-shell/commit/8dc34bd714b6b67250094f6be81d0bb1886269f3))


### Dependencies

* bump sigstore/cosign-installer ([643f3dc](https://github.com/non7top/sigstore-shell/commit/643f3dcec1714bbae99af9d11dfe65bc8daa3d17))
* bump the cargo group across 1 directory with 3 updates ([82eef1f](https://github.com/non7top/sigstore-shell/commit/82eef1ff689916ab9ae8ff4c7fe7dcef6434ebb1))

## [0.2.0](https://github.com/non7top/sigstore-shell/compare/v0.1.0...v0.2.0) (2026-11-05)


### Features

* **ext:** Attestation and Log entry links on a verified result ([6d5d6e6](https://github.com/non7top/sigstore-shell/commit/6d5d6e6509b1f223cce878be29b4c2d9e8882bb8))
* **ext:** outcome glyph, top summary, links and copy buttons on the Sigstore tab ([cd6c277](https://github.com/non7top/sigstore-shell/commit/cd6c277819af7369c5e38e2bafc3dc0df781a273))
* **ext:** outcome icons from SVG, links, copy buttons, layout ([14cfe18](https://github.com/non7top/sigstore-shell/commit/14cfe18f37f0c0d86993200d823cc64edf81a10c))
* Provenance tab redesign and one retry on GitHub 5xx ([89dd4de](https://github.com/non7top/sigstore-shell/commit/89dd4dedd40edca84aece9f4bbda58f13bf69889))


### Bug Fixes

* **ext:** cache attestation bundles and re-verify them on every hit ([328aa0d](https://github.com/non7top/sigstore-shell/commit/328aa0d03543cf7f82494ce719a084afaa9c0553))
* **ext:** cache attestation bundles and re-verify them on every hit ([dfb90e6](https://github.com/non7top/sigstore-shell/commit/dfb90e61bc19cb0a8168967d59a05bb765fcc75a))
* **ext:** stack the tab layout from its content, repo and commit on one line ([24ebb93](https://github.com/non7top/sigstore-shell/commit/24ebb931a7799a27dced4a87e8887582bc700dec))


### Dependencies

* bump jiff from 0.2.38 to 0.2.39 in the cargo group ([f5caeb5](https://github.com/non7top/sigstore-shell/commit/f5caeb581c173da14ab12e69129ea100f74767bd))
* bump rustls from 0.23.45 to 0.23.46 ([da6e8d7](https://github.com/non7top/sigstore-shell/commit/da6e8d734c55e698eae4a3c2922eee6be4f8e2e0))

## 0.1.0 (2026-10-08)


### Features

* build, attest and publish releases from this repo ([73f4b4d](https://github.com/non7top/sigstore-shell/commit/73f4b4d357381cf8d6ef1b989f5273989a3bfc02))
* command-line prototype ([a8eb6db](https://github.com/non7top/sigstore-shell/commit/a8eb6dbc171f5c899dbf5d6461557472638a2193))
* command-line prototype that verifies an exe's Sigstore attestation ([b12285d](https://github.com/non7top/sigstore-shell/commit/b12285dde9d414900ed9ee98f02aa05f0434a276))
* ELF-note provenance claim for the Linux CLI ([f69a1b5](https://github.com/non7top/sigstore-shell/commit/f69a1b5d318cab73343de095ff3934f2c5db507c))
* embed the provenance claim as an ELF note in the Linux CLI and read it ([f84f4b4](https://github.com/non7top/sigstore-shell/commit/f84f4b4ef36a6230f2199f769381772c96332494))
* Explorer property-sheet extension with Windows cross-build and Wine smoke test ([0187175](https://github.com/non7top/sigstore-shell/commit/018717540e98cf6a6d3d6e4ff45784d4ae6aff77))
* NSIS installer replacing the Inno Setup script ([443da80](https://github.com/non7top/sigstore-shell/commit/443da80e07ab25b02ee38006225b29dcbbff7d32))
* NSIS installer shipped with each release ([fe6b8fa](https://github.com/non7top/sigstore-shell/commit/fe6b8fae8209ff46b2df1ea99e854dee548d638a))


### Bug Fixes

* build and link artifacts on PRs, repair the release workflow ([088a45b](https://github.com/non7top/sigstore-shell/commit/088a45be854a1c45924df1d55b708dbedc525358))
* build and link artifacts on PRs, repair the release workflow, add workflow lint ([1fd2e93](https://github.com/non7top/sigstore-shell/commit/1fd2e93e8d592758e526feb69a629a709f00119a))
* stop release-please editing Cargo.toml, take the release version from the tag ([a0d468b](https://github.com/non7top/sigstore-shell/commit/a0d468be8addf4b0041b21a98f5bc5e1f912bab1))
* stop release-please editing Cargo.toml, take the release version from the tag ([3839fde](https://github.com/non7top/sigstore-shell/commit/3839fdefc250d721e24d904f96580592b4a85cb7))
