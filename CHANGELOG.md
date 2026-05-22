# Changelog

All notable changes to this crate are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
