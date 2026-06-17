# oxideav-indeo

Pure-Rust Intel Indeo (IV2 / IV3 / IV4 / IV5) video codec family for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework. Built
from clean-room specification and behavioural-trace documents under
`docs/video/indeo/` only — no external codec source is consulted.

## Status

This crate is a **clean-room scaffold in progress**, focused on Indeo 3
(`IV31` / `IV32`). It does **not** yet produce decoded pixels from a
real bitstream end to end. What is implemented and unit-tested:

- **Frame + bitstream header** (`spec/01`) — the 64-byte combined header
  parse via `indeo3::FrameHeader::parse`.
- **Picture layer** (`spec/02`) — per-plane prelude parsing, plane
  iteration order, half-pel scaling, the packed-MV formula, the typed
  plane-data byte map, the strip-context slot geometry, and the
  picture-layer → per-plane decode-call bridge.
- **Macroblock layer** (`spec/03`) — the MSB-first bit reader and the
  binary-tree walk over a plane's payload, producing a typed `CellTree`
  of INTRA / INTER leaf cells.
- **Reconstruction primitives** (`spec/04` – `spec/07`) — the byte-level
  entropy / mode-byte surface, the per-position dyad-pair output kernel
  and the four cell-shape variant emitters, the in-cell predictor chain
  row driver, VQ_NULL copy-upper / mark-edge executors, the §3.2
  mode-byte jump-table dispatch, packed-MV decode + four-way
  motion-compensation dispatch and cell-copy kernels, strip-edge fix-up,
  the §4.3 / §5.5 / §5.6 / §5.7 output upshift + 4:1:0 chroma
  box-upsampler + strip-to-frame assembly, and
  the spec/06 byte-level mode-byte entropy surface — the literal /
  RLE-escape classification, the two §3.2 high-nibble jump tables, the
  §4 escape-code dispatch with its §4.3 per-position acceptance matrix
  and §4.4 `0xFB` counter byte, and the §3.3 variable-byte continuation
  rule including the per-row continuation-byte lookahead offset, plus
  the spec/07 §5.3 output-format dispatch decision — the `sub_4190`
  selection over input `biCompression` (`IF09` / `BI_RGB` /
  `BI_BITFIELDS`) and output `biBitCount`, resolving to one of seven
  conversion variants with its entry RVA (the RGB variants' §5.4
  LUT-driven bodies remain deferred; only the IF09 passthrough is
  landed), and the spec/07 §6 frame-finalisation state updates — the
  saved `frame_flags` slot (`+0x434`) whose bit-9 drives the next
  frame's reference-bank ping-pong, the saved `frame_number` slot
  (`+0x474`) and the next-frame continuity classifier
  (`incoming == saved + 1` → sequential, else seek), the four
  `sub_4190` return dispositions (`0` / `-100` / `1` / per-plane
  fault), and the §6.4 "no explicit buffer rotation" invariant.

Each stage operates on caller-supplied inputs (cells, deltas, pixel
buffers) and stops at its documented chapter boundary; they are not yet
wired together into a full decode loop.

### Remaining gaps to a real-bitstream decode

- The VQ codebook-bank per-entry values (the `+0x000` / `+0x100` /
  `+0x200` / `+0x300` / `+0x700` banks the cell unpackers index into) —
  pending an extraction round.
- The §7.3 "first bit `1`" VQ-data-without-index unpacker dispatch.
- The §5.4 YUV→RGB output LUT contents.
- A staged `IV31` / `IV32` bitstream fixture to drive the full pipeline.

Indeo 2 / 4 / 5 have only wiki-snapshot documentation under
`docs/video/indeo/indeoN/wiki/` (no formal `spec/`), so they remain at
the round-0 scaffold pending docs work.

## Selected public API

- `indeo3::FrameHeader::parse` — 64-byte combined header (`spec/01`).
- `indeo3::PictureLayer::parse` — per-plane prelude (`spec/02`).
- `indeo3::PictureLayer::plane_byte_map` / `plane_decode_plan` — typed
  plane-data byte ranges and the per-plane decode plan.
- `indeo3::decode_plane_tree` — binary-tree walk → `CellTree`
  (`spec/03`).
- `indeo3::upshift_7bit_to_8bit` / `assemble_plane_if09` — output-stage
  upshift and strip-to-frame assembly (`spec/07` §4.3 / §5.7).
- `indeo3::upsample_chroma_4x4` — §5.5 4:1:0 → output box-filter chroma
  upsampler (replicate each chroma sample into a 4×4 output block;
  `CHROMA_UPSAMPLE_FACTOR`).
- `indeo3::FrameFinalisation::finalise` — spec/07 §6 per-frame state
  updates: `SavedFrameFlags` / `SavedFrameNumber` (the `+0x434` /
  `+0x474` slots), `FrameContinuity::classify` (next-frame continuity),
  and `DecodeReturn` (the four `sub_4190` return codes).
- `alt_quant_indices(byte) -> (primary, secondary)` — §3.9.
- Header constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `PLANE_COUNT`, `MIN_DIMENSION`, `MAX_WIDTH`, `MAX_HEIGHT`, and the
  per-plane / per-vector field widths.

The crate forbids `unsafe` (`#![forbid(unsafe_code)]`); paths beyond the
implemented header + picture-layer surface return
`Error::NotImplemented`.

## License

MIT.
