# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2](https://github.com/OxideAV/oxideav-indeo/compare/v0.0.1...v0.0.2) - 2026-05-06

### Other

- prepend retirement notice (docs audit 2026-05-06)
- release v0.0.1

## [0.0.1](https://github.com/OxideAV/oxideav-indeo/releases/tag/v0.0.1) - 2026-05-03

### Other

- drop duplicate semver_check key
- replace never-match regex with semver_check = false
- cargo fmt: fix rustfmt --check CI gate
- drop nested [workspace] block (umbrella sweep)
- round 2 — wire 143-entry Huffman + four delta tables
- bootstrap oxideav-indeo (Intel Indeo family) — round 1: Indeo 2 scaffold
- Initial commit

### Added
- **Round 2 — Indeo 2 entropy decode lands.** The 143-entry canonical
  Huffman codebook (§8.1 of
  `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`) and
  the four 256-byte delta / palette tables (§8.3) are now wired into
  the decoder. Pair codewords resolve against the active table; run
  codewords expand to 2..32-pixel runs. Intra row 0 uses the absolute
  palette; intra rows ≥ 1 add a signed delta to the row above; inter
  rows add a 3/4-scaled delta on top of the previous frame's pixel.
  Plane order on the wire is Y, V, U.
- Sub-modules `v2::tables`, `v2::huffman`, and `v2::plane` separate
  the static numerical data, the canonical-Huffman lookup builder,
  and the per-plane pair / run pixel emitter.
- New integration test `tests/psnr_against_ffmpeg.rs` cross-checks
  the decoder against `ffmpeg`-decoded reference YUV frames extracted
  from `VPAR0019.AVI`. Y-plane is bit-exact (PSNR = ∞) against the
  reference for all 10 fixture frames; merged YUV PSNR is ≈ 55 dB
  (the small chroma loss is the cost of replicating yuv410p chroma
  into yuv420p without the upsampling filter ffmpeg picks).

### Changed
- `common::BitReader` now reads bits **LSB-first within each byte**,
  matching FFmpeg's `BitstreamContextLE` reader against which the
  trace doc's canonical Huffman codes were derived. This is the
  bit-order Indeo 2 actually uses.
- `Indeo2Decoder` retains a single shared `HuffTable` built once at
  construction and reused across every frame.

### Removed
- Round-1 mid-grey luma / neutral chroma placeholder pixels — the
  pair / run entropy decoder writes real pixel values from the first
  decoded frame.

### Roadmap
- **Round 3 — Indeo 3 (`IV31` / `IV32`).** Block-based VQ codec.
  Pending its own clean-room trace document under
  `docs/video/indeo/indeo3/`.
- **Round 4 — Indeo 4 (`IV41` / `IV42`).** Wavelet codec. Pending
  `docs/video/indeo/indeo4/`.
- **Round 5 — Indeo 5 (`IV50`).** Wavelet codec with motion
  compensation. Pending `docs/video/indeo/indeo5/`.

### Initial scaffold (round 1)
- Initial scaffold of the `oxideav-indeo` crate covering the whole
  Intel Indeo video codec family in a single crate.
- Indeo 2 frame-header parser per
  `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`,
  little-endian bit-reader for the entropy payload, structurally
  correct `yuv420p` `VideoFrame` shape.
- Module layout (`v2`, future `v3`/`v4`/`v5`) and shared `common`
  helpers.
