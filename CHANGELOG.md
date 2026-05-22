# Changelog

All notable changes to this crate are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
