# Icons

Outcome icons shown beside the verdict on the Provenance tab. The text next to each icon carries the meaning; the icon only reinforces it.

| File | Used for | Origin |
| --- | --- | --- |
| `Kliponious-green-tick.svg` | verified | "Green tick - simple" by Kliponius, Openclipart, 2012 |
| `failed.svg` | mismatch, verification failed (red cross) | original to this repository |
| `neutral.svg` | no attestation (grey dash; this does not mean the file is unsafe) | original to this repository |
| `warning.svg` | lookup failed (grey warning triangle) | original to this repository |

## Green tick

- Title: Green tick - simple
- Creator: Kliponius
- Publisher: Openclipart, 2012
- Source: https://openclipart.org/detail/167549/green-tick---simple-by-kliponius
- Also published at: https://freesvg.org/kliponious-green-tick
- Licence: Public Domain (http://creativecommons.org/licenses/publicdomain/), as stated in the file's own metadata and on those pages.

The file is included unmodified. The licence statement rests on that metadata and on the pages above; it is not independent legal advice.

## Our own icons

`failed.svg`, `neutral.svg` and `warning.svg` are drawn for this project and released under the project's MIT licence.

## How they get into the DLL

`crates/sigstore-shell-ext/build.rs` renders each SVG with `rsvg-convert` (Debian package `librsvg2-bin`, installed in the Dockerfile dev stage) at 16, 20, 24, 32, 48 and 64 px, centred on a square canvas so the non-square tick is not stretched. It writes an `.ico` with PNG entries into `OUT_DIR` and embeds it as an icon resource through `winresource`. The generated `.ico` files are not committed. At run time `LoadImage` picks the closest size for the display DPI.
