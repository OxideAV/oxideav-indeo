# oxideav-indeo

Pure-Rust Intel Indeo (IV2 / IV3 / IV4 / IV5) video codec family for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework. Built
from clean-room specification and behavioural-trace documents under
`docs/video/indeo/` only — no external codec source is consulted.

## Status

This crate is a **clean-room scaffold in progress**, focused on Indeo 3
(`IV31` / `IV32`). The structural decode layers are now wired into a
single end-to-end driver — `indeo3::decode_frame` threads the spec/01
header, the spec/02 picture layer + per-plane decode plan, and the
spec/03 binary-tree cell walk into one pass, producing a typed
`DecodedFrame` (per-plane geometry + `CellTree` + per-class cell
statistics) for every present plane in spec/02 §8 decode order. The
spec/07 output stage is wired on top via `indeo3::assemble_output`,
which runs the §5.7 strip-to-frame assembly (7-bit → 8-bit upshift,
edge-marker clear, tight repacking) over per-plane strip pixel buffers
into `OutputFrame` rasters.

The crate does **not** yet produce decoded **pixels** from a real
bitstream: the per-cell reconstruction that fills the strip pixel
buffers (spec/04 §3.2 cell-state dispatch → §3.3 codebook-bank lookup)
needs the **codebook-bank per-entry values** (`bank[+0x000]` /
`[+0x200]` / `[+0x300]` / `[+0x700]` LUTs), which are an Extractor
docs-gap per `spec/04 §7.1` (audit-corrected against
`audit/00-report.md §3`/§4): those tables are zero on disk and built at
codec-init by `IR32_32.DLL!0x100060de`, with the exact per-entry recipe
for several of them still undetermined. The end-to-end driver therefore
stops at that boundary, reporting `ReconstructionStatus::StructureComplete`
once every present plane's cell tree is resolved.

What is implemented and unit-tested:

- **End-to-end structural driver** — `indeo3::decode_frame` /
  `decode_frame_with_selector` (spec/01 → spec/02 → spec/03), producing
  a `DecodedFrame` with per-present-plane `DecodedPlane` (decode plan,
  cell tree, `PlaneCellStats`); NULL-frame short-circuit; spec/02 §8
  (U, V, Y) decode order.
- **Output-plane assembly** — `indeo3::assemble_output` /
  `allocate_strip_buffers` / `plane_strip_buffer_lengths` (spec/07 §5.6
  / §5.7), producing `OutputFrame` / `OutputPlane` rasters from
  per-plane strip pixel buffers.

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

The spec/01 → spec/02 → spec/03 layers are now wired into one
`decode_frame` pass (and the spec/07 output stage onto it via
`assemble_output`); the spec/04 / spec/05 per-cell reconstruction
primitives still operate on caller-supplied inputs (cells, deltas,
pixel buffers) and stop at their documented chapter boundary, because
the cell-state dispatch they need is gated on the codebook-bank values
below.

### Remaining gaps to a real-bitstream decode

- The VQ codebook-bank per-entry values (the `+0x000` / `+0x100` /
  `+0x200` / `+0x300` / `+0x700` banks the cell unpackers index into) —
  pending an extraction round. **This is the single blocker for
  pixel output**: the end-to-end `decode_frame` driver resolves every
  present plane's cell tree but cannot synthesise pixels without these
  LUTs, and they are zero on disk (built at codec-init by
  `IR32_32.DLL!0x100060de`).
- The §5.1 **high-half**-stream cell-state dispatch tables
  (`0x1003f44c` / `0x1003fd4c` / `0x1003fd50`) sourced from seed offset
  `+0x100`: only the single in-bounds pair is determinable from the
  258-byte `0x1003ed4c` extract (audit/00 §2.2), so the per-record
  layout for `ecx > 0` needs a wider extract. The low-half tables
  (`0x1003f24c` / `0x1003f94c` / `0x1003f950`) are now materialised.
- The §7.3 "first bit `1`" VQ-data-without-index unpacker dispatch.
- The §5.4 YUV→RGB output LUT contents.
- A staged `IV31` / `IV32` bitstream fixture to drive the full pipeline.

Indeo 2 / 4 / 5 have only wiki-snapshot documentation under
`docs/video/indeo/indeoN/wiki/` (no formal `spec/`), so they remain at
the round-0 scaffold pending docs work.

## Selected public API

- `indeo3::decode_frame` / `decode_frame_with_selector` — end-to-end
  structural frame decode (spec/01 → spec/02 → spec/03) → `DecodedFrame`
  (`planes: Vec<DecodedPlane>`, `reconstruction_status`); walks planes
  in spec/02 §8 (U, V, Y) order, NULL-frame short-circuit.
- `indeo3::assemble_output` / `allocate_strip_buffers` /
  `plane_strip_buffer_lengths` — spec/07 §5.6 / §5.7 output-plane
  assembly over per-plane strip pixel buffers → `OutputFrame` /
  `OutputPlane`.
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
- `indeo3::SeedDispatchTables` — spec/04 §5.1 codec-init cell-state
  dispatch tables, rebuilt from the 258-byte `.data + 0x1003ed4c`
  seed (the tables are zero on disk per audit/00 §3.1 and must be
  materialised at init): `build()` + `table_f24c()` (the `0x1003f24c`
  4-byte-stride table) + `table_f94c()` (the `0x1003f94c` / `0x1003f950`
  8-byte-stride table) for the low-half seed stream, plus
  `high_half_pair0()` for the single in-bounds high-half pair.
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
