# oxideav-indeo

Pure-Rust **Intel Indeo** video codec family for the
[oxideav](https://github.com/OxideAV) framework. **One crate covers
the whole family** — Indeo 2, Indeo 3, Indeo 4, and Indeo 5 — rather
than a separate crate per generation. Each version lives in its own
`v2` / `v3` / `v4` / `v5` module behind a single registration entry
point.

## Status

| Version | Codec id  | AVI FourCC(s) | Status |
|--------:|:----------|:--------------|:-------|
| Indeo 2 | `indeo2`  | `RT21`, `IV20` | **decode complete** — Y-plane bit-exact vs. ffmpeg, ≈55 dB merged YUV PSNR (chroma upsample-method drift only) |
| Indeo 3 | `indeo3`  | `IV31`, `IV32` | not yet implemented (round 3 — pending `docs/video/indeo/indeo3/`) |
| Indeo 4 | `indeo4`  | `IV41`, `IV42` | not yet implemented (round 4 — pending `docs/video/indeo/indeo4/`) |
| Indeo 5 | `indeo5`  | `IV50`         | not yet implemented (round 5 — pending `docs/video/indeo/indeo5/`) |

Indeo 2 ships fully decoded: the 143-entry canonical Huffman codebook
and the four 256-byte delta tables (§8 of the trace doc) drive the
pair / run plane reader; intra row 0 uses the absolute palette;
intra rows ≥ 1 add a signed delta to the row above; inter rows add a
3/4-scaled delta on top of the previous frame's pixel. Y plane is
byte-exact against `ffmpeg -i VPAR0019.AVI -pix_fmt yuv420p` for
every frame in the in-tree fixture corpus.

Indeo 3, 4, and 5 land in subsequent rounds via the same crate's
`v3` / `v4` / `v5` modules and additional `register()` entries —
the public API does not change.

## Indeo 2 reference

The Indeo 2 implementation follows
`docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md` in
the oxideav-workspace. That document describes RT21 / IV20 as a
**pixel-domain, prefix-coded, pair-or-run delta codec** with a
48-byte fixed per-frame header, no motion compensation, and `yuv410p`
chroma subsampling (one chroma sample per 4×4 luma block). There is
no DCT, no block transform, no motion vectors — Indeo 2 is essentially
a fancy run-length encoder.

The frame-header parser is bit-for-bit driven by §3.3 of that
document and is exercised against a real Intel-encoded RT21 fixture
(see `tests/`).

## Output

Indeo 2 decodes to an internal `yuv410p` raster. The crate exposes
that as [`PixelFormat::Yuv420P`] by 2×2-replicating each chroma
sample into the corresponding 2×2 chroma block — the closest match
in the current `oxideav-core` pixel-format enum. (`yuv410p` itself
is not yet a `PixelFormat` variant; if/when it is added, the output
mapping will switch to it.)

## Roadmap

- **Round 3 (Indeo 3).** `IV31` / `IV32` block-based VQ codec.
  Pending the trace / reverse-engineering document under
  `docs/video/indeo/indeo3/`.
- **Round 4 (Indeo 4).** `IV41` / `IV42` wavelet codec. Pending
  `docs/video/indeo/indeo4/`.
- **Round 5 (Indeo 5).** `IV50` wavelet codec with motion
  compensation. Pending `docs/video/indeo/indeo5/`.

## License

MIT. See [LICENSE](LICENSE).
