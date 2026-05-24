# Changelog

All notable changes to this crate are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Indeo 3 (IV31 / IV32) output-reconstruction kernel (`spec/07`
  §1 + §2 + §4). New `indeo3::reconstruct` module landing the
  per-position pixel-emission arithmetic that round 5's entropy
  module deferred. `apply_dyad_pair(predictor, primary_delta,
  secondary_word)` reproduces the inner-loop body at
  `IR32_32.DLL!0x10006e0f..0x10006e2e`: the softSIMD
  `predictor + primary_delta` DWORD add, the `jns` high-half
  overflow test, the `xor eax, 0x80008000` back-out followed by the
  16-bit `add ax, [secondary]` continuation fall-back, and the `js`
  fault to error code 2 when the secondary add is still sign-set —
  returned as a `DyadOutcome` (`Primary { pixels }` /
  `Continuation { pixels }` / `Fault`). `predictor_offset` computes
  the §1.1 `[edi - 0xb0]` row-above predictor byte index
  (`PREDICTOR_ROW_STRIDE` = 0xb0 = 176), returning `None` for
  top-of-strip writes whose seed is the constant
  `TOP_OF_STRIP_PREDICTOR` (`0x00`, §1.3 / §9). `SoftSimdSum::add`
  records both 16-bit halves' bit-15 overflow sentinels
  (`low_half_overflow` / `high_half_overflow` / `any_half_overflow`),
  and `jns_taken` exposes the literal §2.1 high-half branch (the
  inverse of spec/06's `continuation_needed`). `pack_predictor` /
  `unpack_pixels` move four pixels in and out of the little-endian
  softSIMD pixel DWORD (§0 / §2.4). The §4.2 7-bit-per-byte range
  (`PIXEL_VALUE_MAX` = 0x7f) and the reserved edge-marker bit
  (`EDGE_MARKER_BIT` = 0x80) are surfaced as constants. 11 new unit
  tests cover the predictor stride / seed constants, the row-above
  offset (including top-row `None`), the per-half sentinel record,
  the `jns` ↔ `continuation_needed` inverse, the primary path
  (in-range, secondary word ignored), the continuation path
  (back-out + secondary add, high-half preserved), the fault path,
  the pixel-DWORD pack/unpack round-trip, and a realistic in-range
  dyad-pair. Per the spec/07 boundary this round lands the
  per-position arithmetic kernel only — not the four cell-shape
  variant inner loops (A–D, §2.2), the strip-buffer assembly, the
  7→8-bit upshift, or the YUV→RGB / IF09 conversion (§5), and not
  motion compensation (`spec/05`). Spec source:
  `docs/video/indeo/indeo3/spec/07-output-reconstruction.md`.

- Indeo 3 (IV31 / IV32) byte-level entropy (`spec/06`). New
  `indeo3::entropy` module turning the per-cell mode-byte stream
  into classified, typed structures — the fourth and last of the
  spec/06 §1 entropy mechanisms (the other three are spec/03 §2
  tree codes, spec/03 §3.4 / spec/04 §3.1 leaf-byte indices, and
  the spec/04 §4 VQ_NULL prefix code). `ModeByte::classify` splits
  a mode byte into a literal dyad index (`0x00..=0xF7`,
  `LiteralMode` carrying the §3.1 high-nibble × 4 jump-table
  offset, the low-nibble × 2048 arena-band base, and the
  low-nibble bit-3 `JumpTable` flavour) or an RLE escape
  (`0xF8..=0xFF`, `RleEscape`). `continuation_needed` models the
  §3.3 variable-byte rule — the dyad sum's sign bit (`jns`) decides
  whether a continuation byte is read, making each literal cost 1
  or 2 bytes — with `apply_continuation_xor` for the
  `xor eax, 0x80008000` back-out and `DyadAddress` for the §3.2
  primary (`+0x400`) / secondary (`+0x402`) dyad offsets within the
  band. The eight RLE escapes (`RleEscape::F8..Ff`) carry their
  §4.1 / §4.2 wiki names and handler RVAs; `RleEscape::accepted_at`
  encodes the §4.3 per-position acceptance matrix (`PositionClass`:
  `0xFB`/`0xFC`/`0xFD` accepted everywhere, `0xFE`/`0xFF` at
  row-starts, `0xF8`/`0xF9`/`0xFA` cell-start-only, narrowing
  across continuations) and `extra_bytes` records that only `0xFB`
  consumes a counter byte. `fb_category_table` builds the §4.4
  256-byte `0xFB` counter-byte category lookup from the spec's
  normative seed ranges (`0x01..=0x1F` → copy `0x04`, `0x21..=0x3F`
  → mark-skipped `0x08`, rest → zero `0x00`); the destination at
  `.data + 0x1004ccd4` is all-zero on disk (per
  `tables/region_1004ccd4.meta`, a heap-resident attach-time copy
  of the static seed at `.data + 0x1003ef4c`), so the table is
  reconstructed from the normative ranges rather than vendored.
  `FbCounter::decode` decomposes the counter into
  `(counter & 0x1F) + 1` cells and the bit-5 copy/skip disposition.
  21 new unit tests cover the literal/escape boundary, nibble
  split + bit-3 jump-table selection, the high-nibble action
  categories, all eight escape round-trips, `0xFB`-only
  extra-byte, the full per-position acceptance matrix
  (first-position-accepts-all + the three narrowing continuations),
  the dyad-address layout, the continuation sign-bit test + XOR,
  the category table + classifier + counter decode, and the
  variant / handler / position-class RVA maps. Per the spec/06 §8
  boundary this round stops at the entropy question — *which* bytes
  the stream consumes and *how* each is classified; the pixel
  emission (dyad → pixel-pair, the `add eax, [esi + 4*edx + 0x400]`
  predictor chain, the `0x7f7f7f7f` mask) is `spec/07`, and motion
  compensation is `spec/05`. Spec source:
  `docs/video/indeo/indeo3/spec/06-entropy.md`.

- Indeo 3 (IV31 / IV32) VQ-codebook materialisation (`spec/04`).
  New `indeo3::vq` module turning the spec/03 VQ_DATA leaf indices
  into the structural codebook state the per-cell unpacker consumes.
  Lands the static dyad-mode delta table `DyadDeltaTable` (the 8 KB
  `.data + 0x1003d088` table, 16 banks × 512 B, indexed
  `(high_nibble << 9) + col` per the dyad handler, vendored verbatim
  from the docs clean-room extract and surfacing the audit-noted
  bank-15 row restriction `DYAD_BANK15_VALID_ROWS = 65`); the packed
  codebook-DWORD decoder `CodebookEntry::decode` (§2.1 mode bit 0 /
  bit 1 → one of four `CellVariant`s, bits 2..31 as a signed `sar 2`
  arena offset); the static codebook seed-dispatch builder
  `seed_dispatch_entries` (§5.1 — 128 `SeedEntry` packed as
  `((al << 8) + bl) << 9` with signed `movsx` source bytes from the
  258-byte `.data + 0x1003ed4c` table); the per-frame arena `VqArena`
  plus the `alt_quant[]` band-selection overlay `apply_alt_quant`
  (§6 — `cb_offset << 11` global bias applied once, then per active
  band a 1 KB primary copy from `seed + high_nibble*128` and a 1 KB
  secondary copy from `seed + low_nibble*2048`, zero bytes skipping
  the band, out-of-range source windows surfaced as
  `VqError::SeedWindowOutOfRange`); and the VQ_NULL runtime sub-code
  classifier `VqNullRuntime::classify` (§4 — first-bit-0/second-bit-0
  copy-upper, 0/1 mark-boundary, first-bit-1 unpacker-dispatch). 17
  new unit tests cover the dyad table load + 512-byte bank stride +
  bank-15 restriction, the mode-bit / signed-offset decode, the
  signed seed packing, the band offsets, the overlay primary /
  secondary / skip / cb_offset-bias / out-of-range / negative-bias
  paths, and the VQ_NULL classification. Per the spec/04 §0 / §8
  chapter boundary this round stops at the materialised codebook
  state: no per-byte mode-byte unpacking, dyad-pair → pixel-pair
  expansion, or RLE escape codes (spec/06), no pixel reconstruction
  (spec/07), no motion compensation (spec/05). The static
  `.data + 0x1003d088` / `0x1003ed4c` tables are vendored into
  `src/indeo3/data/*.hex` (verbatim copies of
  `docs/video/indeo/indeo3/tables/region_*.hex`, with a `#`-prefixed
  provenance header) so the published crate is self-contained. Spec
  source: `docs/video/indeo/indeo3/spec/04-vq-codebooks.md`.

- Indeo 3 (IV31 / IV32) macroblock-layer binary-tree walk.
  `decode_plane_tree(&[u8], &PlanePrelude, plane_width,
  plane_height, is_chroma, FrameFlags)` walks the binary tree that
  lives inside a plane's bitstream payload (the bytes that begin at
  the `bitstream_offset` the picture layer computed) and returns a
  typed `CellTree` of INTRA / INTER leaf cells; INTRA cells carry
  their VQ sub-tree leaves inline as `VqCell`s. Implements every
  spec/03 tree-walk rule: the §2.1 MSB-first sentinel-bit reader
  (modelled with the original decoder's two-cursor scheme — the
  bit buffer drains the current byte while the shared `ebp` cursor
  supplies leaf bytes from the next un-loaded byte, per §6 item 7),
  the §2.2 four 2-bit node codes (`00` H_SPLIT, `01` V_SPLIT, `10`
  INTRA/VQ_NULL leaf, `11` INTER/VQ_DATA leaf), the §3 MC_TREE walk
  over a plane-sized root cell (§3.1) with H_SPLIT halving height
  top-first and V_SPLIT halving width left-first (§3.2), the §3.3
  INTRA → VQ_TREE transition on the same physical cell, the §3.4
  INTER one-byte MV-index read, the §4 VQ_TREE walk, the §4.1
  VQ_NULL leaf plus its additional 2-bit sub-code (`00` copy, `01`
  skip, `10`/`11` → `MacroblockError::InvalidVqNullSubCode` fault
  matching the decoder's return code 3), and the §4.1 VQ_DATA
  one-byte codebook-index read. Per the spec/03 §7 chapter
  boundary the walk stops at the per-leaf index-byte fetch:
  `Cell::Inter` records the raw MV-index byte and `VqLeaf::Data`
  the raw codebook-index byte, with no codebook materialisation
  (spec/04), motion compensation (spec/05), or pixel
  reconstruction (spec/07). Truncation and offset faults surface
  as `MacroblockError::{BitstreamTruncated, LeafByteTruncated,
  BitstreamOffsetOutOfRange, DegenerateSplit}`. `LUMA_STRIP_WIDTH`
  / `CHROMA_STRIP_WIDTH` (spec/02 §4.1, 160 / 40) are exposed for
  the strip-classification consumers. 15 new unit tests cover
  the strip-width constants, MSB-first node decode, single
  INTRA-with-VQ_DATA and single
  INTER leaves (leaf-byte cursor), H_SPLIT / V_SPLIT geometry,
  VQ_NULL copy/skip sub-codes, invalid VQ_NULL sub-codes, nested
  split geometry, all four error variants, odd-dimension halving,
  and the absolute error-offset accounting. Spec source:
  `docs/video/indeo/indeo3/spec/03-macroblock-layer.md`.

- Indeo 3 (IV31 / IV32) picture-layer plane-prelude parser.
  `PictureLayer::parse(&FrameHeader, &[u8])` consumes the same
  codec-frame buffer the header parser saw and returns a typed
  `PictureLayer` with one `PlanePresence` per plane. Implements
  every spec/02 §2/§3 rule that governs the bytes between the
  bitstream header and the binary-tree / VQ payload:
  plane iteration order U → V → Y (§2 count-down), plane skip on
  negative offset (§2 `< 0` interpreted as i32) and on offset
  above `data_size/8` (§2 budget check), `num_vectors` u32 LE
  (§3.1), `mc_vectors[num_vectors]` as two signed bytes per entry
  (§3.2, vertical-then-horizontal byte ordering), per-component
  half-pel arithmetic right shift driven by `frame_flags` bits 4
  and 5 with the shifted-out LSB preserved as the half-pel
  sub-field (§3.3), and a `bitstream_offset` precomputed per
  §3.4 (`plane_base + 4 + 2*num_vectors`) for the spec/03 hand-
  off. `MotionVector::packed_mv` exposes the §3.3 packing
  formula. NULL frames (§1, `data_size == 0x80`) skip the plane
  iteration entirely and surface every plane as
  `PlanePresence::NullFrame`. Buffer-overrun classes are
  represented by `PictureLayerError::PlaneOffsetOutOfRange` and
  `PictureLayerError::MotionVectorArrayTruncated`. 15 new unit
  tests cover NULL frame, INTRA frame (all-zero `num_vectors`),
  INTER frame with distinct per-plane motion vectors, the U → V
  → Y iteration order, both skip variants, the boundary
  `plane_offset == budget` case, all three half-pel scaling
  combinations, the §3.3 packed-MV formula, the §3.4 byte-map
  invariant, and the two overrun error paths. Spec source:
  `docs/video/indeo/indeo3/spec/02-picture-layer.md`.

- Indeo 3 (IV31 / IV32) frame-header parser. `FrameHeader::parse`
  consumes the combined 64-byte header at the start of an Indeo 3
  codec frame (16-byte frame header + 48-byte bitstream header)
  and returns a typed view of every field. Validates the §2.1
  `FRMH` checksum, §2.2 `frame_size > 16` bound, §3.1
  `dec_version == 0x0020`, and §3.2 `YVU9_8BIT` (bit 1) rejection,
  surfacing each failure as a distinct `HeaderError` variant.
  `FrameFlags` provides typed accessors for the named bits
  (PERIODIC_INTRA, INTRA, NEXT_INTRA_HINT, MV_HALFPEL_HORIZ /
  _VERT, DROPPABLE_INTER, BUFFER_SELECTOR) plus an `is_inter`
  helper. The NULL-frame `data_size == 0x80` sentinel is
  recognised by `BitstreamHeader::is_null_frame`. `alt_quant[]`
  is preserved verbatim with an `alt_quant_indices` helper for
  the §3.9 high-nibble / low-nibble VQ-table-index split.
  14 unit tests cover happy path, every error path, the §5 byte
  map, and the `FRMH` magic constant. Spec source:
  `docs/video/indeo/indeo3/spec/01-file-header.md`.

### Changed

- Clean-room rebuild from a fresh orphan `master`. The previous
  implementation was retired by the OxideAV docs audit dated
  2026-05-06; the prior history is preserved on the `old` branch.
  See `README.md` for the rebuild scope and the strict-isolation
  workspace the Implementer rounds will draw from.
