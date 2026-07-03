# oxideav-indeo

[![CI](https://github.com/OxideAV/oxideav-indeo/actions/workflows/ci.yml/badge.svg)](https://github.com/OxideAV/oxideav-indeo/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/oxideav-indeo.svg)](https://crates.io/crates/oxideav-indeo) [![docs.rs](https://docs.rs/oxideav-indeo/badge.svg)](https://docs.rs/oxideav-indeo) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Pure-Rust Intel Indeo (IV2 / IV3 / IV4 / IV5) video codec family for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework. Built
from clean-room specification and behavioural-trace documents under
`docs/video/indeo/` only — no external codec source is consulted.

## Status

This crate is a **clean-room scaffold in progress**, covering Indeo 3
(`IV31` / `IV32`, the most developed) and now bootstrapping Indeo 5
(`IV50`). All truth is drawn from the staged spec / trace docs under
`docs/video/indeo/` only — no external codec source is consulted.

### Indeo 5 (`IV50`)

Indeo 5 is a wavelet-subband codec, structurally distinct from the
VQ-based Indeo 3, and lives in its own `indeo5` module built bottom-up
from the staged spec (`docs/video/indeo/indeo5/spec/`). The header
stack now parses end-to-end:

- `indeo5::BitReader` — the LSB-first 32-bit-accumulator bit reader
  (spec/00 §3, spec/01 §3.1).
- `indeo5::FormatDescriptor` — the spec/01 §2 format-descriptor
  preamble (magic + dimension validation).
- `indeo5::PictureStart` — the spec/01 §3 picture-start triplet (PSC +
  frame_type + frame_number + the §3.4 NULL soft-correction).
- `indeo5::GopHeader` / `FrameHeader` / `BandHeader` — the spec/02
  GOP / frame / band headers (flags, decomposition levels + band
  counts, preset/custom picture dimensions, the `band_info` array,
  Huffman-codebook descriptors, the rv-table correction array, the
  global quantiser, and the opaque-extension loops).
- `indeo5::PictureHeader::parse` — the front door threading the whole
  stack and dispatching by frame type (spec/01 §3.5).
- `indeo5::pic_size` — the spec/02 §1.6 standard picture-size tables.

On top of the header stack, the **entropy + transform primitives** the
per-tile coefficient path will consume now land as self-contained,
spec-backed units:

- `indeo5::Codebook` — the shared Huffman codebook in the **IVI
  prefix form** (Indeo 4 wiki annex A, deferred to by the Indeo 5
  page): row `k` codes are `[k ones][0][xbits[k] extras]` with the
  last row's terminator replaced by an extra bit, so every descriptor
  is an exactly-complete code (Kraft sum = 1) — this **resolves the
  formerly-reported spec/04 Kraft anomaly** (the r338 note's
  anomalous-preset set is exactly what the per-symbol-length
  misreading produces). Extra bits decode MSB-first
  (fixture-arbitrated). `MB_HUFF_PRESETS` / `BLOCK_HUFF_PRESETS`
  vendor the eight preset records per context; all sixteen now build.
- `indeo5::build_level_table` — the spec/04 §3.4 256-byte level
  zig-zag-folded signed-byte lookup the per-block decoder maps codeword
  indices through.
- `indeo5::synth_1d` / `recompose_level` / `recompose_plane` — the
  spec/06 §3/§4 CDF 5/3 (LeGall) wavelet synthesis: the §3.3 lifting
  form with §4.2 mirror-reflection boundaries, the §4.1 separable 2D
  recompose over four band quadrants, and the §3.4 bottom-up multi-level
  plane recompose (0/1/2-level decomposition → full plane).
- `indeo5::build_clip_table` — the spec/06 §5.3 48-byte per-cell
  saturation clipping table (audit-corrected storage per audit/00 §3.3).
- `indeo5::TileGrid` — the spec/02 §4.1/§4.2 per-band tile grid (the
  per-axis `ceil(picture / slice)` count + the per-tile rectangle layout
  with the last-column / bottom-row remainder).

On top of the synthesis primitives, the **spec/08 output-reconstruction
stage** now lands as a self-contained subsystem — the path from a
recomposed signed-16-bit plane to a host YUV buffer, all table-free and
independent of the gated coefficient path:

- `indeo5::ReconstructionPlane` / `bias_and_clamp` / `plane_stride`
  (spec/08 §1.1/§3.3) — the per-pixel signed→unsigned conversion the
  eight per-plane writer kernels share
  (`((coeff + 0x200) >> 2) & 0xff`), over the 32-byte-padded
  reconstruction stride, producing a tightly-packed `OutputPlane`.
- `indeo5::ChromaSubsampling` / `upsample_chroma` (spec/08 §3.5/§5) —
  the 4:1:0 / 4:2:0 chroma dimensions and the top-left-cosited 4×4 / 2×2
  box-filter upsample to luma resolution.
- `indeo5::PlaneRole` / `FramePlanes` (spec/08 §1.1/§1.3) — the `Y, V, U`
  record layout and the `U → V → Y` output-writer iteration order.
- `indeo5::OutputFormat` (spec/08 §2.2/§2.3/§5.3) — the five-way
  host-format FOURCC routing (`IF09`/`YVU9`, `YUY2`, `YV12`, `I420`,
  RGB), the `[ebx+0x70]` selector, and the per-format chroma layout /
  planar plane order (incl. the I420 U/V swap).
- `indeo5::pack_planar` (spec/08 §5.3/§6.2) — the planar host-buffer
  concatenation with the per-plane byte-offset triple.
- `indeo5::assemble_frame` (spec/08 §1/§3.3/§5/§6.2) — the whole-frame
  top-level thread: three signed reconstruction planes → chroma-geometry
  validation (§5.1) → per-plane bias-and-clamp in `U → V → Y` order →
  packed planar `HostBuffer`.
- `indeo5::parse_frame_checksum` / `parse_band_checksum` (spec/08 §7) —
  the `frm_checksum` / `band_checksum` parse-and-store (never verified,
  "debugging only").
- `indeo5::DecodeReturn` / `reference_rotation` / `output_row_order`
  (spec/08 §6.3/§8) — the `ICDecompress` return codes, the per-frame
  reference-buffer rotation, the bit-26 output-written flag, and the
  top-down (YUV) / bottom-up (RGB) row order.
- `indeo5::tables` (spec/05 §4.1 / spec/06 §5.1 / spec/08 §3.2,
  audit-corrected) — the numeric static tables extracted from the
  binary's on-disk `.data` regions (Extractor round 9 / Auditor
  round 10): `VLC_END` `[2, 4, 8, 12]`, the `[6, -7, 42]` wavelet-synth
  constants, and the 60-entry `DEQUANT_SCALE_BITS` per-codebook FP scale
  table (byte-exact IEEE-754 bit patterns).

The **spec/07 motion-compensation chapter** has also opened up — its
table-free structural layer is landed:

- `indeo5::Mv` / `resolve_mv` / `MvPredictor` (spec/07 §2/§3) — the
  one-per-MB packed MV layout (Δy/Δx signed bytes + `delta_present`),
  the §2.2 half-pel fold into the `ecx & 3` kernel selector (`McMode`,
  no true quarter-pel), and the §3.2 left-neighbour-only spatial
  predictor with the §3.3 zero-MV tile-entry reset.
- `indeo5::mc_add_block` / `mb_uses_mc` (spec/07 §5) — the four MC
  kernels over the band-coefficient layer (full-pel add, half-pel X/Y
  two-tap average, 2D half-pel four-sample average) with the §5.5
  residual-add semantics and the §5.6 `& 0xc` no-MC transform-id gate.
- `indeo5::RefSlots` (spec/07 §1.2/§1.3/§4) — the reference-frame
  two-frame ping-pong: the eight `[ebx+0xf4..0x114]` slots with the
  pre-/post-decode per-frame-type rotation dispatches, including the
  §1.5 droppable invariant (DROPPABLE_INTER never becomes a future
  reference).

The **spec/03 tile-and-macroblock layer** is landed in full for its
staged scope, and the pipeline's middle is now threaded end-to-end:

- `indeo5::TileHeader` / `TileDataSize` / `tile_predictor_active` /
  `explicit_size_matches` (spec/03 §2) — the 4-stage
  `value24..value27` per-tile data-size prefix code (empty / implicit
  / 8-bit / escape-extended 24-bit), the §2.7 predictor-context flag
  (intra force-clear), and the §2.8 reconciliation check.
- `indeo5::MbGrid` / `Macroblock` / `MbBlock` (spec/03 §3) — the
  per-tile MB grid (ceil counts, raster iteration, boundary clamp,
  block raster layout) plus the vendored four-block coordinate /
  block-stride / band-flags tables (`.rdata 0x10088bf0..0x10088c58`).
- `indeo5::MbHeader` / `Cbp` / `QdeltaMode` / `effective_mb_quant`
  (spec/03 §4, spec/04 §5.2/§5.3, spec/06 §5.2) — the per-MB header
  in §4.5 field order: skip flag, three-mode qdelta, MV-delta pair,
  and CBP, with the header VLCs zig-zag-folded to signed values.
- `indeo5::RvTable` / `escape_lindex` / `run_advance` (spec/05
  §2/§4.2) — the per-band run-value mechanism: parallel
  `RV_TABLE_SLOTS` static contents (the r338 `0x100972f4`
  extraction), the composite `(run, val)` decode, `rv_tab_corr`
  entry swaps, and the three-symbol escape aggregation.
- `indeo5::slant` (spec/06 §1/§2) — the SWAR paired-16-bit
  butterfly primitives (`ror 1`/`ror 2`/`ror 0x11`, the `0x7ffc7ffc`
  / `0xfff8fff8` masks), the §2.1 eight-cluster handler taxonomy, the
  §2.3 page-0 handler-to-slot scan table, representative fragment
  kernels, and the §2.4 transform-variant dispatch selection.

On top, **whole frames now decode to pixels**:

- `indeo5::decode_intra_picture` — the spec/02 §4.4 per-frame walk:
  picture header → per-plane band headers → per-tile size headers →
  the two-phase tile walk (all MB headers, then all coded blocks'
  `(run, val)` coefficient streams — the fixture-arbitrated split) →
  wavelet recompose (LL-innermost band order) → spec/08
  bias-and-clamp + planar pack into a `HostBuffer`. **Both staged
  real `IV50` INTRA fixtures decode end-to-end**: every band's
  entropy stream is consumed to byte-exact exhaustion (`BandTrace`),
  with 1096 coded blocks censused on the 320x240 frame. Zero-
  coefficient regions produce exact pixels (mid-grey); decoded
  coefficients are structurally validated but reconstruct as zero
  pending the scan/dequant/Slant numeric staging. The only remaining
  frontier is `MvInheritance` (inter tiles in `mv_inherit` bands).
- `indeo5::Indeo5Decoder` — the multi-frame session: GOP carry, the
  spec/07 §1.2 per-band reference workspace, NULL repeat-previous
  (spec/08 §6.4), the §3.4 frame-number soft-correction, spec/08
  §8.1 reference promotion (with the droppable no-promote invariant
  and the DROPPABLE_INTER_SCAL chroma swap), and a structural INTER
  decode whose per-MB walk drives the spec/07 band-coefficient-layer
  predictor: zero-MV tile-entry reset, skip-inherits-left MV, and
  the MC copy through `mc_add_block` for decoded non-zero MVs.

What is **not** yet implemented for Indeo 5: pixel reconstruction of
the decoded coefficients — the per-band scan order, the
`band_glob_quant` dequantisation scales, and the 8-point inverse
Slant equations are not yet numerically staged in `docs/` (see the
docs-gaps below), so coded-block regions reconstruct as zeros while
their entropy streams are fully decoded and validated; the per-tile
MV-inheritance fast path (spec/07 §3.4/§3.5 — needs the per-band
`0x3604`/`0x3664` tables) also remains gated. Indeo 5 is decode-only
and not yet registered into the codec registry.

### Indeo 5 reported docs-gaps

Resolved this round (r388, arbitrated against the two staged `IV50`
INTRA fixtures — every alternative reading fails the byte-exact
band-exhaustion test):

- **Preset Huffman-descriptor Kraft anomaly** — resolved: the
  descriptors are IVI prefix-form `xbits` rows, not per-symbol code
  lengths (see `indeo5::Codebook`). spec/04 §1.3/§3.2's
  literal-length builder description is an erratum.
- **Per-band rv-table contents** — resolved by the r338 static
  extraction (`tables/rv_tables_100972f4.*`), transcribed as
  `indeo5::RV_TABLE_SLOTS` with the composite decode semantics
  documented in `indeo5::rv_table` (A = per-run magnitude counts,
  B = vlc→composite permutation, 0/1 = EOB/ESC markers, values
  arranged around per-run interval midpoints; `rv_tab_corr` pairs
  swap entries).
- **Per-tile explicit-size semantics** (spec/03 §2.4 vs §2.8) —
  resolved for §2.8: the count spans the whole tile from its first
  byte (three independent byte-exact tile→band-end chains in the
  320x240 fixture).
- **spec/03 §4.5 field order** — erratum: the CBP precedes the
  qdelta VLC (wiki `value31` < `value33` order), and the
  single-block CBP flag sense is `1 = coded` (§4.3 case-A pseudocode
  is inverted). The qdelta/MV VLCs ride the frame-level MB codebook,
  not the band's block codebook.
- **Tile payload layout** — the per-tile stream is two-phase (all MB
  headers, then all coded-block streams; the Indeo 4 wiki
  "Macroblocks info data" / "Blocks data" split), not interleaved
  per MB as a spec/03 §5 reading suggests.
- **Chroma tile-count rule** (spec/02 §4.1 vs spec/03 §1.1) —
  behaviourally resolved by the r338 fixtures for the un-sliced
  config (chroma tiles independently per band); the multi-slice
  formula stays open.

Still open (the numeric material needed for pixel-exact
reconstruction of coded coefficients):

- **Scan order, dequantisation and the fused inverse Slant**
  (spec/05 §5.1, spec/06 §2/§5). The per-band scan tables, the
  `band_glob_quant`→scale mapping, and the per-handler butterfly
  equations are structurally described but not numerically staged;
  decoded `(run, val)` streams are therefore validated and counted
  (`DecodeStats`, `BandTrace`) but reconstruct as zero. An Extractor
  round staging the scan tables + the dequant base tables (and a
  Specifier pass on the 8-point Slant equations) closes this.
- **DC handling for intra blocks.** The 240x180 black-frame fixture
  decodes with *no* coefficients anywhere (the vendor decoder
  reproduces `Y=16, U=V=128` for it), so the zero state is pinned;
  how non-zero DC content is carried (in-stream at scan position 0
  vs a differential side channel per the Indeo 4 annex-B wording)
  needs a fixture-backed trace.
- **Escape value fold + over-256 symbols.** The ESC path's 3-VLC bit
  structure is pinned by the fixtures (two emissions), the value
  fold is provisional; custom codebooks wider than 256 symbols
  (e.g. the 320x240 Y band's 269) have no rv-table mapping for their
  tail symbols.
- **Band trailing tails.** The 320x240 fixture's bands leave 3-8
  non-zero bytes unconsumed inside the explicit tile counts
  (`BandTrace` pins them); whether the vendor decoder reads them is
  undetermined.
- **Vendor output conversion.** The staged `expected.yuv` files are
  the sandbox harness's packed-4:2:2 view of the vendor decoder's
  RGB24 output (the black frame lands at `Y=16, U=V=128`), so
  byte-exact output comparison additionally needs the spec/08 §3.7
  YUV→RGB LUT contents (unstaged) and the harness's RGB→YUV
  convention.

### Indeo 3 (`IV31` / `IV32`)

The structural decode layers are now wired into a
single end-to-end driver — `indeo3::decode_frame` threads the spec/01
header, the spec/02 picture layer + per-plane decode plan, and the
spec/03 binary-tree cell walk into one pass, producing a typed
`DecodedFrame` (per-plane geometry + `CellTree` + per-class cell
statistics) for every present plane in spec/02 §8 decode order. The
spec/07 output stage is wired on top via `indeo3::assemble_output`,
which runs the §5.7 strip-to-frame assembly (7-bit → 8-bit upshift,
edge-marker clear, tight repacking) over per-plane strip pixel buffers
into `OutputFrame` rasters.

The **genuinely-unblocked subset** of reconstruction now runs whole-frame:
`indeo3::reconstruct_frame` walks a `DecodedFrame`'s present planes,
materialises every VQ_NULL unit (copy-upper + mark-edge, spec/07 §1.4 /
§4.4) into a real strip pixel buffer, and surfaces the precise
`(x, y, disposition)` frontier where the path first hits a gated unit;
`ReconstructedFrame::to_output_frame` upshifts those strips into an
`OutputFrame`. What that pass **cannot** yet synthesise is the **VQ_DATA**
cells: their per-cell reconstruction (spec/04 §3.2 cell-state dispatch →
§3.3 codebook-bank lookup) needs the **codebook-bank per-entry values**
(`bank[+0x000]` / `[+0x200]` / `[+0x300]` / `[+0x700]` LUTs), an Extractor
docs-gap per `spec/04 §7.1` (audit-corrected against
`audit/00-report.md §3`/§4): those tables are zero on disk and built at
codec-init by `IR32_32.DLL!0x100060de`, with the exact per-entry recipe
for several of them still undetermined. INTER cells additionally need a
prior decoded reference frame. So for a real frame the output carries the
VQ_NULL regions' pixels with the VQ_DATA / INTER regions left black until
those gates clear; the structural driver still reports
`ReconstructionStatus::StructureComplete` once every present plane's cell
tree is resolved.

What is implemented and unit-tested:

- **`oxideav-core` framework integration** (`indeo3::registry`) — the
  Indeo 3 decoder is wired into the framework's published codec surface
  so a pipeline resolving codecs through an `oxideav_core::CodecRegistry`
  (the way the container crates do) can construct and drive it without
  naming this crate's concrete types. `Indeo3RegistryDecoder` implements
  `oxideav_core::Decoder` over the stateful `Indeo3Decoder`, mapping each
  decoded frame's full-luma-resolution YUV (spec/07 §5.5 box-upsampled
  chroma) into a `Yuv444P` (Y, U, V) `VideoFrame`; `make_decoder` +
  `register_codecs` / `register` install the codec (id + caps + factory +
  probe + the `IV31` / `IV32` FourCC tags) and the crate-root
  `oxideav_core::register!` wires zero-config fleet registration.
  `codec_id_for_fourcc` + `probe` give the FourCC routing surface a
  demuxer's `CodecResolver` needs — the probe validating a first packet's
  `spec/01 §2.1` combined-header `check_sum` (never touching the
  docs-gapped codebook-bank values). `decode_video_frame(data, pts)` is
  the one-shot direct-API counterpart. Decoder-only (no encoder). The
  bridge re-shapes exactly the genuinely-unblocked VQ_NULL subset the
  decoder already produces; VQ_DATA / INTER regions stay black pending
  the codebook-bank docs-gap.
- **End-to-end structural driver** — `indeo3::decode_frame` /
  `decode_frame_with_selector` (spec/01 → spec/02 → spec/03), producing
  a `DecodedFrame` with per-present-plane `DecodedPlane` (decode plan,
  cell tree, `PlaneCellStats`); NULL-frame short-circuit; spec/02 §8
  (U, V, Y) decode order.
- **Output-plane assembly** — `indeo3::assemble_output` /
  `allocate_strip_buffers` / `plane_strip_buffer_lengths` (spec/07 §5.6
  / §5.7), producing `OutputFrame` / `OutputPlane` rasters from
  per-plane strip pixel buffers.
- **Full-resolution YUV frame** — `indeo3::assemble_yuv` /
  `upsample_frame` (spec/07 §5.5 over §5.7), producing a `YuvFrame` /
  `YuvPlane` whose three planes (Y, V, U) are all at full luma
  resolution: the §5.5 box-filter upsamples each 4:1:0 chroma plane
  4×4 onto the §5.7-assembled output. This is the §5.4-RGB-independent
  half of the output-conversion stage — the exact luma-resolution
  three-plane surface the §5.4 YUV→RGB matrix consumes per pixel,
  producible without the (zero-on-disk / docs-gapped) `0x1004cxxx`
  YUV→RGB LUTs.

- **Static-table-only per-cell reconstruction executor** —
  `indeo3::reconstruct_cell_static` (`spec/06` §3 / §4 + `spec/07` §1 /
  §3) is the crate's first **mode-byte stream consumer**: given a cell's
  geometry and the byte slice the per-cell unpacker reads from `[ebp]`,
  it walks the cell row by row, classifies each mode byte
  (`ModeByte::classify`), and drives a strip pixel buffer through the
  handlers that need **only on-disk tables** — the high-nibble-0
  row-band-advance handler (`apply_row_band_seed` over the vendored
  `.data + 0x1003d088` dyad table; `spec/07` §3.2), the RLE skip escapes
  `0xFD` / `0xFE` / `0xFF` (`spec/06` §4.2), the `0xFB` counter-byte
  terminator (§4.4), and the start-of-cell edge-mark family `0xF8` /
  `0xF9` / `0xFA` / `0xFC`, with the §4.3 per-position acceptance matrix
  enforced (mis-positioned escapes return `EscapeFault`, the binary's
  error-code-1 return). A literal mode byte whose **high nibble is
  non-zero** addresses the per-frame VQ codebook arena (the `spec/04`
  §7.1 codebook-bank docs-gap): rather than guess, the walk stops and
  returns `CellOutcome::DeferredArena` with the exact mode byte +
  (row, dword) position — the cleanest boundary report for the next
  Extractor round. This is the first piece of the reconstruction path
  that genuinely *produces strip-buffer pixels from a mode-byte stream*
  (for the static-table-only subset), as opposed to operating on
  caller-supplied deltas.
- **Plane-level reconstruction-readiness classifier** —
  `indeo3::classify_cell_tree` / `classify_plane` (`spec/03` §3 / §4 +
  `spec/04` §3 / §4 + `spec/05`) walks a `DecodedPlane`'s cell tree and
  maps every reconstruction unit (each INTER leaf, each VQ sub-cell of an
  INTRA leaf) to a `CellDisposition`: VQ_NULL copy / skip (table-free,
  reconstructable now), VQ_DATA (the `spec/04` §7.1 codebook-bank
  docs-gap), or INTER (motion compensation, needs a reference frame). The
  aggregate `DispositionCounts` reports per plane how many units the
  unblocked subset covers (`unblocked()`) versus how many wait on a
  docs-gap / reference frame (`deferred()`) — a measured reconstruction
  roadmap over the structural decode. `drive_vq_null_copies` then
  *executes* the genuinely-unblocked half: every VQ_NULL copy unit is
  driven through `copy_upper_cell` over a strip pixel buffer (`spec/07`
  §1.4 literal upper-row copy, no table input), producing real strip
  pixels.
- **Whole-plane reconstruction executor** — `indeo3::exec_plane_plan`
  (`spec/07` §1.4 / §4.4 / §5.1) is the plane-spanning successor to
  `drive_vq_null_copies`. It sizes a `plane_height × 0xb0` strip pixel
  buffer (the §1.3 zero-fill seed) from a `PlaneReconstructPlan`, walks
  every reconstruction unit in plan order, and drives both VQ_NULL arms:
  copy → `copy_upper_cell`, skip → `mark_edge_cell` (the §4.4 bit-7
  edge marker), each one four-row band at a time so an 8-row cell drives
  two bands. VQ_DATA / INTER units are counted and the first one is
  recorded as a `DeferredFrontier` — the exact `(x, y, disposition,
  entry_index)` where the unblocked path first stops on the codebook-bank
  docs-gap / a missing reference frame. The returned `ReconstructedPlane`
  carries the mutated strip plus a `PlaneExecStats` coverage report
  (`reconstructed()` / `deferred()` / `bytes_written` /
  `is_fully_reconstructed()`), turning the per-cell primitives into one
  whole-plane pixel-synthesis pass over the unblocked subset.
- **Frame-level reconstruction pass** — `indeo3::reconstruct_frame`
  (`spec/07` §1.5 / §5.2) threads `exec_plane_plan` across a
  `DecodedFrame`'s present planes (in U, V, Y decode order, exploiting
  the §1.5 per-plane independence), reconstructing each plane's unblocked
  subset and folding every plane's coverage into one frame-wide
  `FrameReconstructStats`. The returned `ReconstructedFrame` carries one
  `ReconstructedPlane` per present plane (its strip + frontier) — a
  single whole-frame entry point drivable straight off `decode_frame`'s
  output, with the codebook-bank / motion-compensation frontier surfaced
  per plane. `ReconstructedFrame::to_output_frame` bridges the
  reconstructed strips into an `OutputFrame` of tightly-packed 8-bit
  planes via the §4.3 upshift (`(b & 0x7f) << 1`), closing the
  reconstruct → assemble loop over the actually-reconstructed pixels
  (deferred VQ_DATA / INTER regions upshift to black).
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
- **Static-dyad row-band-advance handler** (`spec/07` §3.1 / §3.2) —
  `indeo3::apply_row_band_seed` / `DyadDeltaTable::row_band_delta`
  realise the high-nibble-0 cell-unpacker handler at
  `IR32_32.DLL!0x10006c14`, the one per-cell path that reads the
  (fully-extracted) static dyad table at `.data + 0x1003d088` directly:
  it resolves the §3.1 index `(high_nibble << 9) + row*4 + col`, reads
  the signed delta byte, and writes it into the predictor slot
  `[edi - 0xb0]` (with the `0x80` sign-bias re-applied) to seed the next
  row's prediction.
- **Per-frame codebook seed-area parser** (`spec/04` §5.2) —
  `indeo3::CodebookSeedArea` vendors the static seed table at
  `.data + 0x1004d26a` and walks its §5.2 variable-length block
  structure (count `N` + `N` signed byte-pairs, `0`-count terminator),
  surfacing each `SeedBlock` and the §5.2 step-3a `(b<<8)|a`-with-`0x80`-
  bias `<<16` packing (`SeedPair::primary_dword`). This is the producer
  side of the §6 `alt_quant[]` overlay; the final materialised seed
  window it would feed is blocked on the §5.2 / audit/00 §2.3 block-format
  contradiction (see gaps below).

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
- The **per-frame VQ arena values** — the §6 `alt_quant[]` overlay is
  implemented (`VqArena::apply_alt_quant`) and its raw seed source is
  now parsed (`CodebookSeedArea`, §5.2), but the §5.2 codec-init walk
  that materialises the overlay's `static_seed` window is blocked by a
  **spec-vs-audit contradiction** on the `.data + 0x1004d26a` block
  format: `spec/04 §5.2` reads it as count-prefixed blocks (leading
  `0xc3` ⇒ a 391-byte first block), while `audit/00 §2.3 / §6.5` walks
  the same bytes as zero-gap-delimited records (record 1 = 92 B at
  offset 3) and states the leading `0xc3` is **not** a length prefix.
  The two readings are mutually incompatible, so the per-band →
  arena-offset assignment is undetermined. Resolving this needs a
  Specifier/Auditor pass reconciling §5.2 with the audit's empirical
  record structure (and ideally a wider extract past the 4 KB window,
  audit/00 §6.2).
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
- `indeo3::classify_cell_tree` / `classify_plane` — plane-level
  reconstruction-readiness classifier (`spec/03` + `spec/04` §3 / §4) →
  `PlaneReconstructPlan` (`entries: Vec<CellPlanEntry>` + `DispositionCounts`);
  `drive_vq_null_copies` executes the VQ_NULL-copy subset over a strip
  buffer → `VqNullDriveStats` / `PlaneReconstructError`.
- `indeo3::exec_plane_plan` — whole-plane reconstruction executor
  (`spec/07` §1.4 / §4.4 / §5.1) → `ReconstructedPlane` (mutated strip +
  `PlaneExecStats` coverage + `DeferredFrontier`); sizes the strip via
  `plane_strip_len` (`STRIP_ROW_STRIDE` = `0xb0`), drives both VQ_NULL
  arms (copy + skip) and surfaces the first VQ_DATA / INTER frontier.
  `PlaneExecError` completes the surface.
- `indeo3::reconstruct_frame` — frame-level reconstruction pass
  (`spec/07` §1.5 / §5.2) over a `DecodedFrame` → `ReconstructedFrame`
  (`planes: Vec<ReconstructedPlane>` + `FrameReconstructStats`); runs
  `exec_plane_plan` on every present plane and folds frame-wide coverage.
  `FrameReconstructError` tags the failing plane.
- `indeo3::DecodeSession` — multi-frame decode session / inter-frame
  state machine (`spec/01` §3.2 / §3.3 / §3.6 / §4 + `spec/07` §6).
  `admit(input) -> Result<AdmittedFrame, SessionError>` threads the
  spec/07 §6.1 / §6.2 saved `frame_flags` / `frame_number` slots across
  a frame sequence and classifies each incoming frame into a
  `FrameAdmission` (`FirstFrame` / `Sequential` / `NullRepeat` / `Seek`),
  enforcing the first-frame + seek INTRA requirement, the NULL-frame
  repeat-previous path, and the previous-frame-bit-9 reference-bank
  ping-pong (`AdmittedFrame::read_bank` / `return_value` / `continuity`).
  Table-free — not gated on the codebook-bank docs-gap. `next_read_bank`
  / `saved_frame_number` expose the carried state.
- `indeo3::Indeo3Decoder` — stateful multi-frame decoder
  (`spec/01` §3 + `spec/07` §1.5 / §5.2 / §6). `decode(input) ->
  Result<DecodedOutput, DecoderError>` joins the `DecodeSession`
  sequencer to `decode_frame` + `reconstruct_frame`, holding the
  previous `ReconstructedFrame` so a **NULL-repeat** frame re-emits the
  prior output (spec/07 §6.3) while a picture-carrying frame is freshly
  reconstructed. `DecodedOutput` carries the `AdmittedFrame`, a
  `repeated_previous` flag, and a borrow of the reconstructed frame, with
  `to_output_frame()` / `to_yuv_frame()` one-call paths to a displayable
  `OutputFrame` / full-luma-resolution `YuvFrame`. Reconstructs the
  unblocked (VQ_NULL) subset; inter-frame sequencing is table-free.
- `indeo3::reconstruct_cell_static` — static-table-only per-cell
  mode-byte executor (`spec/06` §3 / §4 + `spec/07` §1 / §3) → `CellOutcome`
  (`Complete` / `DeferredArena` / `Terminated`) over a strip pixel
  buffer; `CellReconstructGeometry` / `PositionEffect` /
  `CellReconstructError` complete the surface.
- `indeo3::assemble_output` / `allocate_strip_buffers` /
  `plane_strip_buffer_lengths` — spec/07 §5.6 / §5.7 output-plane
  assembly over per-plane strip pixel buffers → `OutputFrame` /
  `OutputPlane`.
- `indeo3::assemble_yuv` / `upsample_frame` — spec/07 §5.5 over §5.7:
  full-resolution `YuvFrame` / `YuvPlane` (Y carried through, V / U
  box-upsampled 4×4 to luma resolution). The §5.4-RGB-independent half
  of the output-conversion stage.
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
