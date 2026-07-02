# Changelog

All notable changes to this crate are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Indeo 5 (`IV50`) decoder finalisation** (`indeo5::finalise`,
  `spec/08 §6.3`/`§8.1`/`§8.2`/`§8.5`) — the post-host-write cleanup
  decisions. `DecodeReturn` models the three `ICDecompress` return codes
  (`Ok`=0, `BadFormat`=2, `FrameSkipped`=5, `spec/08 §8.5`);
  `reference_rotation(frame_type)` maps the `spec/08 §8.1` 4-entry jump
  table (`Promote` for INTRA/INTER, `PromoteWithChromaSwap` for
  DROPPABLE_INTER_SCAL, `NoPromote` for DROPPABLE_INTER / NULL);
  `frame_produces_output` gates the NULL no-output path (`spec/08 §6.4`);
  `OUTPUT_WRITTEN_FLAG`/`mark_output_written`/`is_output_written` model
  the `spec/08 §8.2` `[ebx+0x128]` bit-26 fast-skip guard; and
  `output_row_order(format)` gives the `spec/08 §6.3` row order
  (top-down for YUV, bottom-up for RGB per the BMP `biHeight`
  convention). Table-free; reuses the `spec/01` `FrameType`. 5 new unit
  tests (lib count 925 → 930).
- **Indeo 5 (`IV50`) frame / band checksum parse-and-store**
  (`indeo5::checksum`, `spec/08 §7`) — the wiki-documented
  `frm_checksum` / `band_checksum` fields, which the shipping decoder
  reads and stores but **never verifies** ("debugging purposes only",
  `spec/08 §7.3`). `frame_checksum_present(frame_flags)` gates on
  `frame_flags` bit 4 (`FRAME_CHECKSUM_FLAG` `0x10`);
  `parse_frame_checksum` reads the gated 16-bit LSB-first value (no bits
  consumed when absent); `parse_band_checksum` reads the per-band 1-bit
  `checksum_flag` then the optional 16-bit value. `ChecksumField::in_range`
  models the `spec/08 §7.1` `cmp .., 0xffff; ja error` range guard and
  `enforced()` is always `false` (store-only, `spec/08 §7.4`). No
  checksum arithmetic. 6 new unit tests (lib count 919 → 925).
- **Indeo 5 (`IV50`) planar host-buffer packing** (`indeo5::pack`,
  `spec/08 §5.3`/`§6.2`) — the per-plane writers' planar concatenation.
  `pack_planar(planes, format)` lays the three reconstructed planes into
  a `HostBuffer` in the format's `spec/08 §5.3` plane order — `Y, V, U`
  for `Yvu9`/`Yv12`, `Y, U, V` for `I420` (the U/V swap) — each at its
  native resolution, and records the per-plane byte-offset triple
  (`spec/08 §3.6` `[ebx+0x10..0x18]`) as `PlanePlacement`s;
  `HostBuffer::plane_bytes(role)` locates any plane. The packed `Yuy2`
  (`Y0 U Y1 V` sampling deferred, `spec/08 §9.4`) and RGB (LUT-gated,
  `spec/08 §9.1`) formats return `None`. 6 new unit tests (lib count
  913 → 919).
- **Indeo 5 (`IV50`) output-format dispatch** (`indeo5::format`,
  `spec/08 §2.2`/`§2.3`/`§5.3`) — the host-output-format routing.
  `OutputFormat` (`Yvu9`/`Yuy2`/`Yv12`/`I420`/`Rgb`) with
  `from_fourcc(biCompression)` routes the five FOURCCs the
  `ICDecompressBegin` dispatch accepts (`IF09`/`YVU9` → `Yvu9`, `YUY2`,
  `YV12`, `I420`/`IYUV`, `BI_RGB` → `Rgb`); the FOURCC constants
  (`FOURCC_IF09` `0x39304649` … `FOURCC_YUY2` `0x32595559`) are vendored
  and ASCII-verified. `selector()`/`from_selector()` map the codec-
  instance `[ebx+0x70]` value (1..5, `spec/08 §2.2`); `chroma_layout()`
  gives the `spec/08 §5.3` host layout (4:1:0/4:2:0 planar, 4:2:2 packed,
  RGB); `subsampling()` the decode-internal ratio; `plane_order()` the
  `spec/08 §5.3` planar order with the I420 U/V swap vs YV12. The RGB
  pixel conversion stays gated on the docs-gapped YUV→RGB LUT
  (`spec/08 §9.1`) — this module routes to `Rgb` but does not convert.
  9 new unit tests (lib count 904 → 913).
- **Indeo 5 (`IV50`) output-stage plane record set + iteration order**
  (`indeo5::planes`, `spec/08 §1.1`/`§1.3`) — the three-plane output
  bookkeeping. `PlaneRole` (`Luma`/`ChromaV`/`ChromaU`) with
  `record_index()` maps the `spec/08 §1.1` `Y, V, U` record layout
  (`[ebx+0x17c]`/`[ebx+0x1b0]`/`[ebx+0x1e4]`); `PLANE_RECORD_ORDER` and
  `OUTPUT_ITERATION_ORDER` vendor the layout order and the `spec/08 §1.3`
  reverse `U → V → Y` writer walk. `num_bands(levels)` gives the per-plane
  band count `3·levels + 1` (`spec/02 §1.5`: 1/4/7) that selects the
  `spec/08 §3.1` writer path. `FramePlanes` holds the three
  `OutputPlane`s and `iter_output_order()` yields them in the `U → V → Y`
  order the host writer consumes. 6 new unit tests (lib count 898 → 904).
- **Indeo 5 (`IV50`) output-stage chroma subsampling + upsampling**
  (`indeo5::chroma`, `spec/08 §3.5`/`§5.1`/`§5.2`) — the chroma output
  path. `ChromaSubsampling` models the two ratios the GOP `gop_flags`
  bit 1 selects (`Yvu9` 4:1:0, the dominant `chroma_levels = 0` mode;
  `Yv12` 4:2:0); `chroma_dims(luma_w, luma_h)` derives the subsampled
  plane size `ceil(luma / scale)` (`spec/08 §5.1`, the `+3`/`+1`
  rounding bias). `upsample_chroma` box-filter-replicates a subsampled
  chroma `OutputPlane` up to luma resolution: every luma position
  `(x, y)` reads chroma `(x >> shift, y >> shift)` with no interpolation
  (`spec/08 §5.2` top-left-cosited box filter, sharp step at the
  subsample boundary), rejecting a mis-sized chroma plane. 8 new unit
  tests (lib count 890 → 898).
- **Indeo 5 (`IV50`) output-stage per-plane bias-and-clamp**
  (`indeo5::output`, `spec/08 §1.1`/`§3.3`) — the first landed piece of
  the `spec/08` output-reconstruction chapter (previously code-less).
  After wavelet recomposition (`spec/06 §3`) each plane is a signed
  16-bit per-pixel reconstruction buffer; the eight per-plane writer
  kernels share the `spec/08 §3.3` conversion
  `output_byte = ((coeff + 0x200) >> 2) & 0xff` (the `+512` signed→
  unsigned recentre + 10-bit→8-bit downshift, no explicit saturation).
  `bias_and_clamp(coeff)` is the per-pixel kernel; `plane_stride(width)`
  is the `spec/08 §1.1` 32-byte-padded reconstruction stride
  `(width + 0x1f) & ~0x1f`; `ReconstructionPlane` carries the padded
  signed buffer and `to_output_plane()` applies the conversion to every
  visible sample while dropping the right-edge stride padding, yielding
  a tightly-packed `OutputPlane`. Operates on caller-supplied
  reconstruction buffers (the wavelet-synthesis contract), independent
  of the gated coefficient-decode path. 8 new unit tests (lib count
  882 → 890).
- **Indeo 5 (`IV50`) per-band tile geometry** (`indeo5::tile`, `spec/02
  §4.1`/`§4.2`) — the structural tile grid each band is partitioned into
  before per-tile coefficient decode. `tile_count(picture_dim,
  slice_dim)` derives the per-axis count `ceil(picture / slice)`
  (`spec/02 §4.1`: 352×288 / 64 → 6×5); `TileGrid::build(band_w, band_h,
  count_x, count_y)` lays out the per-tile `(col, row, x, y, width,
  height)` rectangles in `spec/02 §4.4` raster order, with the `spec/02
  §4.2` last-column / bottom-row remainder so the tiles cover the band
  exactly; `TileGrid::tile(col, row)` does the grid lookup. The per-tile
  coefficient data-size header (`spec/02 §4.3`) + the coefficient stream
  are the gated `spec/03+`/`spec/05+` scope and are not parsed here. 10
  new unit tests (lib count 872 → 882).
- **Indeo 5 (`IV50`) per-cell saturation clipping table**
  (`indeo5::clip_table`, `spec/06 §5.3`, audit-corrected per `audit/00
  §3.3`) — the 48-byte per-MB clipping lookup built at the per-block
  prologue (`IR50_32.DLL!0x1001f421..0x1001f452`) that saturates a
  reconstructed coefficient into the displayable pixel band.
  `build_clip_table(clip_input)` materialises the §5.3 loop
  (`clamp(clip_input + eax + 0x18, 0, 0x17)`, un-bias, centre on `0x80`)
  over the counter `eax ∈ [-0x30, 0)`; `clip_lookup` does the masked
  lookup. The `clip_input` (per-MB combined shift
  `band_glob_quant + mb_qdelta`, `spec/06 §5.2`) is a parameter — its
  derivation from a real bitstream rides the gated coefficient path, but
  the table-build given a known `clip_input` is exact; no docs gap in
  the builder. 6 new unit tests (lib count 862 → 868).
- **Indeo 5 (`IV50`) multi-level plane recomposition** (`indeo5::wavelet::
  recompose_plane` + `LevelBands`, `spec/06 §3.4`/`§4.1`) — the bottom-up
  multi-level orchestration over `recompose_level`: given the innermost
  LL band plus the per-level `(hl, lh, hh)` high-frequency triples
  ordered innermost-to-outermost, it folds each level's three HF bands
  into the running low-low band (innermost first), doubling both plane
  axes per level. A 0-level plane returns its single LL band verbatim
  (`spec/06 §3.4`), a 1-level plane doubles once, a 2-level plane doubles
  twice — matching the `spec/02 §1.5` `3·levels + 1` band count. 4 new
  unit tests.
- **Indeo 5 (`IV50`) CDF 5/3 wavelet recomposition** (`indeo5::wavelet`,
  `spec/06 §3`/`§4`) — the LeGall 5/3 synthesis filter that recomposes a
  plane's `1 + 3·levels` wavelet bands back into the plane-resolution
  buffer. `synth_1d(low, high)` runs the `spec/06 §3.3` lifting form
  (even update `e[i] -= (h[i-1] + h[i] + 2) >> 2`, odd update
  `o[i] += (e[i] + e[i+1]) >> 1`) with `spec/06 §4.2` mirror-reflection
  whole-sample boundary extension, doubling the sample count;
  `recompose_level(ll, hl, lh, hh)` runs the `spec/06 §4.1` separable 2D
  synthesis (row-pass then column-pass) over four band quadrants into a
  full-resolution `Band`. This covers the §3/§4 synthesis, which is
  fully specified independently of the gated, entropy-fused per-block
  inverse Slant (`spec/06 §2`, whose per-codebook scale tables +
  handler enumeration are an Extractor docs-gap, `spec/06 §6` items
  1/2/3/7): it consumes already-inverse-transformed band buffers, the
  exact contract the §3.5 out-of-place synthesis routine implements. 10
  new unit tests (lib count 852 → 862).
- **Indeo 5 (`IV50`) level zig-zag table** (`indeo5::level_table`,
  `spec/04 §3.4`, audit-corrected per `audit/00 §3.2`) — the shared
  256-byte level-magnitude lookup the per-block coefficient decoder
  consumes (`spec/04 §4.3`). `build_level_table()` materialises it from
  the §3.4 recurrence (odd `i` → `-0x80 - i/2`, even `i` → `i/2 - 0x80`,
  truncated to a signed byte so the over-`-128` odd values fold into the
  positive range — the "zig-zag fold"); `level_value` does the masked
  lookup. The on-disk bytes are zero (PE-loader zero-fill) so the table
  is built at runtime from the fully-specified algorithm — no docs gap.
  6 new unit tests (lib count 846 → 852).
- **Indeo 5 (`IV50`) canonical-Huffman codebooks** (`indeo5::codebook`,
  `spec/04 §1`/`§3.2`/`§4.3`) — the shared entropy primitive the
  per-MB header VLCs (`spec/03 §4`) and the per-block coefficient stream
  (`spec/05`) both invoke. `Codebook::build` performs the standard
  left-to-right canonical-Huffman assignment from a per-row bit-length
  descriptor (a `0` length is a skipped symbol hole, `spec/04 §3.2`),
  producing the ascending `(length, code)` codeword set; `Codebook::decode`
  walks an LSB-first `BitReader` one bit at a time, matching the running
  codeword against the assigned set (a prefix-free guarantee makes the
  first match unique). `from_preset` / `from_huff_desc` route a
  `HuffDesc` (preset id / inline custom / implicit default 7) to a
  codebook, and `HuffContext` carries the mb-vs-block context.
  `MB_HUFF_PRESETS` (Table A, `spec/04 §1.5`) and `BLOCK_HUFF_PRESETS`
  (Table B, `spec/04 §1.4`) vendor the eight preset row-length records
  per context as documented numeric data. **Reported docs gap:** the
  preset records as listed in §1.4/§1.5 are not Kraft-valid per-row
  bit-length codebooks (scaled Kraft sums ≠ `2^max` for most records),
  and the §3.2 builder — itself documented as deduced from `mov`
  patterns, with a 4-byte table entry carrying up to three symbols per
  10-bit prefix — uses a non-plain-prefix-free assignment whose exact
  code-space rule needs a dump of the populated 4 KB table (`spec/04 §6`
  item 8, an Extractor-round subject). `build` therefore implements the
  standard rule (correct for the encoder's inline custom descriptors)
  and reports `Oversubscribed` for a non-Kraft-valid descriptor rather
  than inventing the multi-symbol semantics; `raw_preset` / `kraft_scaled`
  expose the records + the validity diagnostic for the cross-check. 14
  new unit tests (`cargo test -p oxideav-indeo` lib count rises to 846,
  was 832).
- **Indeo 5 (`IV50`) decode bootstrap** (`indeo5` module) — the
  wavelet-based Indeo 5 codec begins its clean-room decode stack from
  the staged spec under `docs/video/indeo/indeo5/spec/`, built bottom
  up. Indeo 5 is structurally distinct from the VQ-based Indeo 3, so it
  gets its own module. Landed:
  - `indeo5::BitReader` — the LSB-first 32-bit-accumulator bit reader
    (`spec/00 §3`, `spec/01 §3.1`): whole-DWORD prefetch seed, low-end
    extraction, one-byte top-end refill, `align(8)`, and the
    bits-consumed bookkeeping the downstream "bytes consumed"
    out-parameter needs.
  - `indeo5::FormatDescriptor` — the `spec/01 §2` format-descriptor
    preamble validator (dual-magic acceptance + in-place normalisation,
    height-before-width dimensions, `>= 4` + multiple-of-4 constraints).
  - `indeo5::PictureStart` — the `spec/01 §3` picture-start triplet
    (5-bit PSC + 3-bit frame_type + 8-bit frame_number) with the §3.4
    duplicate-`frame_number` soft-correction to NULL and the §3.3
    illegal frame_type rejection.
  - `indeo5::pic_size` — the `spec/02 §1.6` standard picture-size
    tables (numeric values from `tables/region_10088c90_u32.csv` /
    `region_10088cd0_u32.csv`, including the audit-confirmed index-3 =
    704×480 correction).
  - `indeo5::GopHeader` — the `spec/02 §1` GOP header: `gop_flags`,
    `slice_size_id`, `decomp_levels` (with the `{0,1,2,6}` gate +
    band-count derivation), `pic_size_id` (preset lookup or custom
    13+13-bit dimensions), subsampling-driven chroma dimensions, the
    per-band `band_info` array (with `mb_size`/`blk_size` tables +
    `ext_trans` transform selector), and the transparency block.
  - `indeo5::FrameHeader` — the `spec/02 §1.9 + §2` frame header: the
    GOP trailer + `gop_ext` loop (INTRA), `frame_flags`, conditional
    `pic_hdr_size` / `frm_checksum` / `frm_hdr_ext` / `mb_huff_desc`
    (`HuffDesc` preset vs custom), `value5`, and the alignment exit.
  - `indeo5::BandHeader` — the `spec/02 §3` per-band header:
    `band_flags`, empty-band fast path, frame-gated `band_data_size`,
    the rv-table correction array (`<= 61` bound), `rv_tab_sel`,
    `blk_huff_desc`, `band_checksum`, `band_glob_quant`, and the
    `band_hdr_ext` loop.
  - `indeo5::PictureHeader::parse` — the front door threading the whole
    header stack and dispatching by frame type (`spec/01 §3.5`).

  Decoder-only, no pixel reconstruction yet: the per-tile coefficient
  stream, the inverse Slant transform, and wavelet recomposition
  (`spec/05`-`spec/08`) are the next milestones.

- **One-shot direct decode** (`indeo3::decode_video_frame(data, pts) ->
  Result<VideoFrame>`) — the direct-API counterpart to the registry
  path, mirroring the `decode_*` free-function convention sibling codec
  crates follow. Builds a fresh `Indeo3Decoder`, decodes `data` as the
  first (INTRA) frame, and shapes the output into a `Yuv444P` (Y, U, V)
  `oxideav_core::VideoFrame`. A non-INTRA first frame fails the
  `spec/01 §3.2` first-frame gate with `Error::invalid`; callers
  decoding a sequence (where inter-frame state / NULL-repeat / the
  reference-bank ping-pong matter) use the stateful
  `Indeo3RegistryDecoder` / `Indeo3Decoder` instead.
- **Indeo 3 tag-disambiguation probe** (`indeo3::probe`,
  `oxideav_core::ProbeFn`) attached to the codec registration. When the
  demuxer has peeked a first packet, the probe validates the Indeo 3
  combined-header `check_sum` (`frame_number ^ unknown1 ^ frame_size ^
  'FRMH'`, the `spec/01 §2.1` check) plus the §2.2 `frame_size > 16`
  floor — reading only the fixed 16-byte frame header, never the
  docs-gapped codebook-bank values. A valid header returns
  `PROBE_CONFIDENCE_HEADER_OK` (so a genuine Indeo 3 payload out-ranks a
  colliding FourCC claimant), a present-but-invalid packet returns `0.0`
  (lets a colliding claimant win on non-Indeo-3 bytes), and no-packet
  returns `PROBE_CONFIDENCE_TAG_ONLY` (the FourCC match alone is decent
  evidence). Wired into `register_codecs` via `CodecInfo::probe`, so the
  registry's `resolve_tag` path now validates `IV31` / `IV32` claims
  against actual frame bytes.
- **`oxideav-core` codec-registry integration** for Indeo 3
  (`indeo3::registry`, re-exported at the `indeo3` and crate roots).
  Bridges the in-crate stateful `Indeo3Decoder` to the framework's
  published codec surface so a pipeline that resolves codecs through an
  `oxideav_core::CodecRegistry` — the way the container crates do — can
  construct and drive an Indeo 3 decoder without naming this crate's
  concrete types. New surface:
  - `codec_id_for_fourcc(&[u8; 4]) -> Option<CodecId>` maps an on-wire
    FourCC (`IV31` / `IV32`, case-insensitive; the two in-scope FourCCs
    per `spec/00-scope.md`) to the `"indeo3"` codec id, so a demuxer's
    `CodecResolver` routes a video track here. `INDEO3_FOURCCS` /
    `CODEC_ID_STR` expose the tag set + id.
  - `Indeo3RegistryDecoder` implements `oxideav_core::Decoder`: owns an
    `Indeo3Decoder`, feeds each `Packet`'s bytes through it, and maps the
    resulting full-luma-resolution `YuvFrame` (spec/07 §5.5 box-upsampled
    chroma) into an `oxideav_core::VideoFrame` in `PixelFormat::Yuv444P`
    plane order (Y, U, V). `flush` → EOF, `reset` restarts the
    inter-frame INTRA gate after a container seek. NULL / repeat frames
    re-emit the previous output (spec/07 §6.3) via the underlying decoder.
  - `make_decoder` (the `DecoderFactory`), `register_codecs(&mut
    CodecRegistry)` / `register(&mut RuntimeContext)` install the codec
    (id + capabilities + factory + the `IV31` / `IV32` FourCC tags); the
    crate-root `oxideav_core::register!("indeo", register)` wires
    zero-config fleet registration through `oxideav-meta`. Decoder-only
    (no encoder) — this is a clean-room decode rebuild. The bridge adds
    no new decode behaviour: it reconstructs exactly the
    genuinely-unblocked VQ_NULL subset the decoder already produces
    (VQ_DATA / INTER regions stay black pending the `spec/04 §7.1`
    codebook-bank docs-gap) and merely re-shapes the output.
- Indeo 3 (IV31 / IV32) stateful multi-frame decoder
  (`indeo3::Indeo3Decoder` → `DecodedOutput` with `DecoderError`,
  spec/01 §3 + spec/07 §1.5 / §5.2 / §6). Joins the `DecodeSession`
  sequencer to the structural decode + reconstruction so a sequence of
  codec frames becomes a sequence of reconstructed `OutputFrame`s. The
  decoder owns the `DecodeSession` *and* the previous
  `ReconstructedFrame`, implementing the spec/07 §6.3 repeat-previous
  semantics: `Indeo3Decoder::decode(input)` admits the frame, and for a
  **picture-carrying** frame (first / sequential / seek) structurally
  decodes (`decode_frame`) + reconstructs (`reconstruct_frame`) it —
  storing the result as the new previous frame — while for a
  **NULL-repeat** frame it re-emits the held previous frame's
  reconstruction byte-for-byte (the session guarantees a NULL frame is
  never first, so a held frame always exists). `DecodedOutput` bundles
  the `AdmittedFrame` classification, a `repeated_previous` flag, and a
  borrow of the reconstructed frame; `to_output_frame()` (spec/07 §4.3 /
  §5.6) and `to_yuv_frame()` (spec/07 §5.5 / §5.6 full-luma-resolution
  three-plane YUV) give a one-call path from a decoded frame to a
  displayable surface, and `read_bank()` / `is_resync_point()` surface
  the reference bank and INTRA-resync status. A rejected frame leaves the
  decoder state (session baseline + held frame) unchanged. Like
  `reconstruct_frame` it reconstructs only the unblocked (VQ_NULL)
  subset of each picture frame — the inter-frame *sequencing* +
  repeat-previous output is the table-free contribution, not gated on
  the codebook-bank docs-gap. 7 new unit tests + a new
  `tests/decode_session.rs` integration suite (6 tests) drive
  `DecodeSession` / `Indeo3Decoder` over multi-frame sequences as a
  downstream consumer would (full INTRA → INTER → NULL → seek sequence,
  NULL repeat-previous byte-equality, the bit-9 read-bank ping-pong, the
  seek-to-INTER reject keeping the held frame, and per-frame
  `to_output_frame` assembly), plus a `to_output_frame` / `to_yuv_frame`
  convenience-passthrough test. `cargo test -p oxideav-indeo` lib count
  rises to 749 (was 740).
- Indeo 3 (IV31 / IV32) multi-frame decode session
  (`indeo3::DecodeSession` → `AdmittedFrame` with `FrameAdmission` /
  `SessionError`, spec/01 §3.2 / §3.3 / §3.6 / §4 + spec/07 §6). The
  first inter-frame state machine: threads one frame's
  `FrameFinalisation` (the spec/07 §6.1 / §6.2 saved `frame_flags`
  `[instance+0x434]` and `frame_number` `[instance+0x474]` slots)
  forward into the next frame's continuity + reference-bank decision,
  so a whole IV31 / IV32 frame sequence can be sequenced without
  re-deriving the inter-frame rules per frame. `DecodeSession::admit`
  parses the spec/01 header and classifies each frame against the saved
  state: the **first frame** must be INTRA (an INTER / NULL first frame
  is the spec/01 §3.2 `-100` input error → `SessionError::FirstFrameNotIntra`);
  a **NULL frame** (`data_size == 0x80`, §3.3) repeats the previous
  output (spec/07 §6.3 return `1` → `FrameAdmission::NullRepeat`); a
  **sequential** frame (`frame_number == saved + 1`, §6.2) admits as
  `Sequential`; a **discontinuous** (seek / gap, §3.6) frame admits as
  `Seek` only when it is INTRA, else `SessionError::SeekNotIntra` (the
  §4 seek path re-validates the INTRA requirement). Each `AdmittedFrame`
  carries the `Bank` the frame reads its previous-frame reference from
  (spec/07 §6.1 / spec/05 §4.2: driven by the *saved* frame's bit-9),
  the spec/07 §6.3 `DecodeReturn`, and the spec/07 §6.2
  `FrameContinuity` classification; `carries_picture()` /
  `is_resync_point()` surface whether the caller should run the
  structural decode + reconstruction and whether the frame begins a
  fresh INTRA reference chain. The §6.4 invariant holds (no decoder-side
  buffer rotation — the bank ping-pong is encoder-driven via bit-9), and
  a rejected frame leaves the continuity baseline un-advanced. This is
  genuinely-unblocked decode progress: the entire frame-sequencing
  contract is table-free (no codebook-bank docs-gap), turning the
  isolated per-frame finalisation primitives into a working multi-frame
  decoder skeleton. 14 new unit tests cover the first-frame INTRA gate
  (INTER / NULL rejection), sequential INTER admission, the NULL-repeat
  + baseline-advance (in-sequence and out-of-sequence), the seek-to-INTRA
  admit vs seek-to-INTER reject, the previous-frame-bit-9 read-bank
  ping-pong, the periodic-intra-vs-INTRA gate distinction, a malformed
  header, and a full INTRA → INTER → NULL → INTER → seek-INTRA sequence;
  `cargo test -p oxideav-indeo` lib count rises to 740 (was 726).
- Indeo 3 (IV31 / IV32) frame-level reconstruction pass
  (`indeo3::reconstruct_frame` → `ReconstructedFrame` with
  `FrameReconstructStats` / `FrameReconstructError`, spec/07 §1.5 / §5.2).
  Threads the whole-plane executor (`exec_plane_plan`) across a
  `DecodedFrame`'s present planes in decode order: classifies each
  plane's cell tree, reconstructs its unblocked (VQ_NULL) subset into a
  strip pixel buffer, and folds every plane's coverage into one
  frame-wide `FrameReconstructStats` (`reconstructed()` / `deferred()` /
  `bytes_written` / `is_fully_reconstructed()`). The result carries one
  `ReconstructedPlane` per present plane (each with its mutated strip +
  per-plane frontier), exploiting spec/07 §1.5 per-plane independence so
  planes reconstruct in isolation. A NULL / fully-skipped frame
  reconstructs to an empty result. This turns the per-plane executor into
  a single whole-frame entry point drivable straight off `decode_frame`'s
  output. `ReconstructedFrame::to_output_frame` then bridges the
  reconstructed strips into an `OutputFrame` of tightly-packed 8-bit
  planes via the spec/07 §4.3 upshift (`(b & 0x7f) << 1`, clearing the
  §4.4 edge-marker sentinel) — closing the reconstruct → assemble loop
  over the *actually-reconstructed* pixels (deferred regions stay black).
  6 new unit tests.
- Indeo 3 (IV31 / IV32) plane-level reconstruction executor
  (`indeo3::exec_plane_plan` → `ReconstructedPlane` with `PlaneExecStats`
  / `DeferredFrontier` / `PlaneExecError`; `plane_strip_len`,
  `STRIP_ROW_STRIDE`, spec/07 §1.4 / §4.4 / §5.1). Sizes a
  plane-spanning strip pixel buffer (`plane_height` rows of `0xb0`
  bytes, the spec/07 §1.3 zero-fill seed) from a `PlaneReconstructPlan`,
  walks every reconstruction unit in plan order, and dispatches each to
  its disposition's executor: VQ_NULL copy → `copy_upper_cell` (literal
  upper-row copy, one four-row band at a time so an 8-row cell drives two
  bands), VQ_NULL skip → `mark_edge_cell` (the §4.4 bit-7 edge-marker
  write). VQ_DATA and INTER units are counted and the first one is
  recorded as the `DeferredFrontier` (`(x, y, disposition, entry_index)`)
  — the exact `(x, y)` where the unblocked path first stops on the
  spec/04 §7.1 codebook-bank docs-gap / a missing reference frame. The
  result carries the mutated strip plus a `PlaneExecStats` coverage
  report (`reconstructed()` / `deferred()` / `bytes_written` /
  `is_fully_reconstructed()`). This is the whole-plane successor to
  `drive_vq_null_copies`: it now also drives the mark-edge skip cells,
  owns the strip-buffer sizing, and surfaces the precise reconstruction
  frontier instead of leaving it to the caller. 9 new unit tests.
- Indeo 3 (IV31 / IV32) plane-level reconstruction-readiness classifier
  (`indeo3::classify_cell_tree` / `classify_plane` → `PlaneReconstructPlan`
  with `CellDisposition` / `CellPlanEntry` / `DispositionCounts`;
  `drive_vq_null_copies` → `VqNullDriveStats` / `PlaneReconstructError`,
  spec/03 §3 / §4 + spec/04 §3 / §4 + spec/05). Walks a `DecodedPlane`'s
  cell tree and maps every reconstruction unit (each INTER leaf, each VQ
  sub-cell of an INTRA leaf) to its disposition: VQ_NULL copy / skip
  (table-free, reconstructable now), VQ_DATA (the spec/04 §7.1
  codebook-bank docs-gap), or INTER (motion compensation, needs a
  reference frame). The aggregate `DispositionCounts` reports per plane
  how many units the unblocked subset covers (`unblocked()`) versus how
  many wait on a docs-gap / reference frame (`deferred()`) — turning the
  structural decode into a measured reconstruction roadmap. The
  genuinely-unblocked half is also *executed*: `drive_vq_null_copies`
  drives every VQ_NULL copy unit through `copy_upper_cell` over a strip
  pixel buffer (spec/07 §1.4, literal upper-row copy, no table input),
  producing real strip pixels. 5 new unit tests.
- Indeo 3 (IV31 / IV32) static-table-only per-cell reconstruction
  executor (`indeo3::reconstruct_cell_static` → `CellOutcome` /
  `PositionEffect` / `CellReconstructGeometry` / `CellReconstructError`,
  spec/06 §3 / §4 + spec/07 §1 / §3). The first end-to-end *mode-byte
  stream consumer*: given a cell's geometry and the byte slice the
  per-cell unpacker reads from `[ebp]`, it walks the cell row by row,
  classifies each mode byte (`ModeByte::classify`), and drives the strip
  pixel buffer through the handlers that need only on-disk tables — the
  high-nibble-0 row-band-advance handler (`apply_row_band_seed` over the
  vendored `.data + 0x1003d088` dyad table; spec/07 §3.2), the RLE skip
  escapes `0xFD` / `0xFE` / `0xFF` (row advances; spec/06 §4.2), `0xFB`
  (counter-byte terminator; §4.4), and the start-of-cell edge-mark family
  `0xF8` / `0xF9` / `0xFA` / `0xFC`. The §4.3 per-position acceptance
  matrix is enforced (`RleEscape::accepted_at` → `CellReconstructError::
  EscapeFault` = the binary's error-code-1 return). A literal mode byte
  whose high nibble is non-zero addresses the per-frame VQ codebook arena
  (the spec/04 §7.1 codebook-bank docs-gap — zero on disk, blocked on the
  §5.2 / audit/00 §2.3 `0x1004d26a` block-format contradiction): rather
  than guess, the walk stops and returns `CellOutcome::DeferredArena`
  with the exact mode byte + (row, dword) position, the cleanest boundary
  report for the next Extractor round. 10 new unit tests.
- Indeo 3 (IV31 / IV32) full-resolution YUV frame producer
  (`indeo3::assemble_yuv` / `upsample_frame` → `YuvFrame` / `YuvPlane`,
  spec/07 §5.5 over §5.7). Wires the §5.5 box-filter chroma upsampler
  onto the §5.7 strip-to-frame assembly: the Y plane is carried through
  at full luma resolution and each present 4:1:0 V / U plane is
  box-upsampled 4×4 (every chroma sample replicated into a 4×4 output
  block, per §5.5's "plain box-filter … no interpolation") onto the
  full luma resolution. The result is the exact three-plane,
  luma-resolution surface the §5.4 YUV→RGB matrix consumes per pixel —
  the §5.4-RGB-independent half of the output-conversion stage,
  producible without the `0x1004cxxx` YUV→RGB LUTs (audit/00 §3.3:
  zero-on-disk / runtime-built at an undetermined RVA — a docs-gap).
  5 new unit tests + a new `tests/yuv_pipeline.rs` integration suite
  (3 tests) driving `decode_frame` → `assemble_yuv` over caller-supplied
  strip buffers and asserting the §4.3 upshift + §5.5 chroma
  upsample-to-luma-resolution end-to-end.
- Indeo 3 (IV31 / IV32) static-dyad-table row-band-advance handler
  (`indeo3::apply_row_band_seed` / `DyadDeltaTable::row_band_delta` /
  `row_band_column`, spec/07 §3.1 / §3.2) — the high-nibble-0 handler
  at `IR32_32.DLL!0x10006c14`, the one cell-unpacker path that reads
  the (fully-extracted) static dyad table at `.data + 0x1003d088`
  rather than the per-frame arena. Resolves the spec/07 §3.1 index
  `(high_nibble << 9) + row_position*4 + column_offset` and writes the
  signed delta byte (with the `0x80` sign-bias re-applied for the
  7-bit-per-byte range) into the predictor slot `[edi - 0xb0]` to seed
  the next row's prediction. 4 new unit tests. This is real pixel-path
  progress that is NOT gated on the per-frame-arena DOCS-GAP.
- Indeo 3 (IV31 / IV32) per-frame codebook seed-area parser
  (`indeo3::CodebookSeedArea`, spec/04 §5.2) — the producer side of
  the spec/04 §6 `alt_quant[]` overlay's `static_seed` input. Vendors
  the static seed table at `.data + 0x1004d26a`
  (`tables/region_1004d26a.hex`, 4 KB window) into the crate and walks
  its spec/04 §5.2 variable-length block structure (1-byte count `N` +
  `N` signed byte-pairs, `0`-count terminator), surfacing each
  `SeedBlock` and the spec/04 §5.2 step-3a packing formula
  (`SeedPair::primary_dword`: `(b<<8)|a` with the `0x80` high-bit bias
  and `<<16` word scale). The final materialised seed window the §6
  overlay copies from is **not** produced: spec/04 §5.2 and audit/00
  §2.3/§6.5 give mutually incompatible readings of the raw block format
  (count-prefixed blocks vs zero-gap-delimited records), so the
  per-band → arena-offset assignment is a reported DOCS-GAP. 8 new
  unit tests.
- Indeo 3 (IV31 / IV32) end-to-end integration tests
  (`tests/end_to_end_structure.rs`) — drive the public
  `decode_frame` → `allocate_strip_buffers` → `assemble_output` chain
  on synthetic frames exactly as a downstream consumer would, covering
  the NULL-frame no-plane path, a single INTRA luma plane threading to
  structure-complete + zeroed-strip assembly, and a malformed-header
  typed error. The README Status section is rewritten to describe the
  now-wired pipeline and pin the single remaining pixel-output blocker
  (the spec/04 §7.1 codebook-bank values).
- Indeo 3 (IV31 / IV32) output-plane assembly driver —
  `indeo3::assemble_output` / `allocate_strip_buffers` /
  `plane_strip_buffer_lengths` wire the spec/07 §5.7 strip-to-frame
  assembly onto the `DecodedFrame` from `decode_frame`. Given the
  per-plane strip pixel buffers (the surface per-cell reconstruction
  fills, gated on the spec/04 §7.1 codebook-bank docs-gap),
  `assemble_output` runs `assemble_plane_if09` for every present
  plane in spec/07 §5.6 output order (`OUTPUT_ASSEMBLE_ORDER` =
  Y, V, U) — upshifting each strip's 7-bit pixels to 8 bits (clearing
  the §4.4 edge-marker bit) and packing them into a tightly-strided
  `OutputPlane` raster, bundled as an `OutputFrame` with `plane()` /
  `luma()` / per-`OutputPlane` `row()` accessors. `allocate_strip_buffers`
  produces correctly-sized zeroed strip sets for a frame (one buffer
  per strip per present plane), so the output path is exercised
  end-to-end against the driver's real geometry; `AssembleError`
  reports strip-count mismatch and per-plane assembly failures with
  the offending `plane_idx`. 5 unit tests cover the strip-length
  geometry, the Y/V/U order, an allocate-then-assemble zeroed round
  trip, the strip-count-mismatch rejection, and a non-zero strip's
  one-bit upshift (`0x09` → `0x12`).
- Indeo 3 (IV31 / IV32) end-to-end structural frame-decode driver —
  `indeo3::decode_frame` / `decode_frame_with_selector` thread the
  previously-disconnected per-stage primitives into one pass over a
  whole codec frame: spec/01 `FrameHeader::parse` → spec/02
  `PictureLayer::parse` → spec/02 §4/§5/§6 `plane_decode_plan` →
  spec/03 `decode_plane_tree`, walked in spec/02 §8 decode order
  (`FRAME_PLANE_DECODE_ORDER` = U, V, Y) with a `const _` cross-check
  against `PLANE_IDX_*`. The result is a typed `DecodedFrame`
  (`header` / `picture` / `planes` / `reconstruction_status`) whose
  per-present-plane `DecodedPlane` bundles the `PlaneDecodePlan`,
  the spec/03 `CellTree`, and a `PlaneCellStats` per-class summary
  (top-level INTRA / INTER counts plus the nested VQ_DATA / VQ_NULL
  sub-cell counts). NULL frames (`data_size == 0x80`,
  `ReconstructionStatus::NullFrame`) short-circuit with no planes;
  non-NULL frames carry every present plane to
  `ReconstructionStatus::StructureComplete`. `FrameDecodeError`
  wraps the per-stage header / picture-layer / per-plane-tree
  errors with the offending `plane_idx`. The driver stops at the
  spec/04 §3.2 cell-state-dispatch boundary: pixel synthesis is
  gated on the codebook-bank per-entry values (`bank[+0x000]` /
  `[+0x200]` / `[+0x300]` / `[+0x700]` LUTs), an Extractor docs-gap
  per spec/04 §7.1 / audit/00 §3–§4. 5 unit tests cover the
  NULL-frame short-circuit, all-skipped-planes, the U/V/Y decode
  order, a single-INTRA-plane walk to structure-complete, and the
  bit-9 buffer-selector read.
- Indeo 3 (IV31 / IV32) spec/04 §5.1 cell-state dispatch-table
  materialisation — `indeo3::SeedDispatchTables` reproduces the codec-init
  static-table init function (entered at `IR32_32.DLL!0x100060de`) that
  builds the cell-state dispatch tables from the 258-byte seed at
  `.data + 0x1003ed4c`. Audit/00 §3.1 confirmed the six destination tables
  (`0x1003f24c` / `0x1003f44c` / `0x1003f950` / `0x1003f94c` / `0x1003fd50`
  / `0x1003fd4c`) are zero on disk and must be rebuilt at init, and §4
  established that `DllMain` runs Path 1 (`0x10006262`) — the path spec/04
  §5.1 quotes (`eax = (al << 8) + bl`, then `eax <<= 9`). `build()`
  materialises the three **low-half**-stream tables fully determined by the
  vendored seed: `table_f24c()` (the `0x1003f24c` 4-byte-stride table, one
  packed DWORD per record) and `table_f94c()` (the `0x1003f94c` / `0x1003f950`
  8-byte-stride table, each record `[packed, packed]` since both `+0x0`/`+0x4`
  halves receive the same DWORD). The three **high-half**-stream tables
  source from seed offset `+0x100`; audit/00 §2.2 notes the 258-byte extract
  covers only the single in-bounds pair, surfaced by `high_half_pair0()`,
  with the remaining records deferred as a DOCS-GAP. `SEED_DISPATCH_RECORDS`
  (= 128) names the per-table record count. 4 unit tests cover the f24c
  packing, the f94c dual-half identity, and the high-half pair-0 pack.
- Indeo 3 (IV31 / IV32) spec/07 §6 frame finalisation — `indeo3::frame_finalise`
  lands the per-frame state-update slice `sub_4190` runs after the §5
  output stage and before its `ret`. `SavedFrameFlags` (§6.1) models the
  16-bit `[outer_instance + 0x434]` slot and its
  `next_frame_read_bank` returns the `Bank` the *next* frame reads from,
  driven by *this* frame's bit-9 `BUFFER_SELECTOR` value (the
  encoder-driven ping-pong, reusing `Bank::from_buffer_selector`).
  `SavedFrameNumber` (§6.2) models the `[outer_instance + 0x474]` slot
  and `FrameContinuity::classify` reproduces the next-frame continuity
  check (`if [eax + 0x474] != ecx`): incoming `== saved + 1` →
  `Sequential`, else `Discontinuous` (the seek path re-validating the
  INTRA requirement), with wrapping `u32` arithmetic. `DecodeReturn`
  (§6.3) enumerates the four `sub_4190` return dispositions (success `0`,
  input error `-100` / `0xffffff9c`, repeat-previous `1`, per-plane fault
  passthrough) with `code()` yielding the exact `i32` the VfW dispatcher
  sees. `PERFORMS_BUFFER_ROTATION` (`= false`, §6.4) records that the
  decoder performs no explicit buffer rotation. `FrameFinalisation`
  bundles the §6.1 / §6.2 / §6.3 outputs so a caller finalises a frame in
  one step and carries the saved-slot pair to the next frame. 15 unit
  tests cover the slot offsets / RVAs, the bit-9 → bank fold, the
  sequential / gap / repeat / reverse / wrap continuity cases, the four
  return codes, fault-return slot capture, and an end-to-end
  finalise → next-frame continuity + bank chain.
- Indeo 3 (IV31 / IV32) spec/07 §5.5 4:1:0 → output chroma box-upsampler
  — `indeo3::frame_output` gains `upsample_chroma_4x4`, the
  `CHROMA_UPSAMPLE_FACTOR` (`4`) ratio constant, and the
  `ChromaUpsampleError` validation enum. This executes §5.5's "plain
  box-filter chroma upsampling": each chroma (V / U) sample is replicated
  into the 4×4 block of output positions it covers (output index `/ 4`
  picks the source, §5.5's "integer division by 4 / shift by 2"), with no
  interpolation, no edge-aware reconstruction, and no chroma plane stride.
  The source is one assembled chroma plane (e.g. the output of
  `assemble_plane_if09`) carrying already-8-bit values; the upsampler
  copies bytes verbatim (the §4.3 upshift is folded into §5.4's LUT /
  applied once per-plane by §5.6, not re-applied here). Source / dest
  stride padding outside the raster is honoured and left untouched. This
  feeds the §5.4 RGB conversion loops whose LUT bodies stay deferred. 13
  unit tests cover single-sample fill, the 2×2 quadrant box pattern,
  source / dest stride padding isolation, the no-upshift verbatim copy,
  zero-dimension no-ops, an end-to-end `assemble_plane_if09` →
  `upsample_chroma_4x4` chain, and the four bounds-validation errors.
- Indeo 3 (IV31 / IV32) spec/07 §5.3 output-format dispatch decision —
  `indeo3::frame_output` gains `select_output_conversion`, the
  `OutputConversion` enum (seven variants), `OutputDispatchError`, the
  `BI_RGB` / `BI_BITFIELDS` input-`biCompression` constants, and the
  `RGB24_STRIDE_FIXUP_BIT_COUNT` trigger. This models the `sub_4190`
  (`0x10004644..0x10004915`) conversion-function-pointer selection that
  installs `var_24` and invokes it via `call [var_24]`: the dispatch
  switches first on the host's *input* `biCompression` (`'IF09'` →
  passthrough; `BI_RGB == 0` → RGB; `BI_BITFIELDS == 3` → palette) and
  then, for the RGB arm, on the *output* `biBitCount` (8 → `0x10008774`
  indexed; 16 → `0x10008a50`; 24 → `0x100096fc` canonical / `0x10009aa0`
  alternate, split by the colour-space flag). `OutputConversion::entry_rva`
  returns each variant's §5.3-table conversion-function RVA, and
  `is_implemented` flags the lone landed body (the IF09 passthrough,
  `assemble_plane_if09`); the RGB variants' §5.4-LUT-driven bodies stay
  deferred until the codec-init LUT-population evidence is staged
  (spec/07 §5.4 audit note + §7.2). 10 new unit tests pin the per-arm
  selection, the colour-space-flag split, the unsupported-compression /
  unsupported-RGB-bit-count fault paths, the exact entry RVAs, and the
  24-bpp stride-fix-up trigger; `cargo test -p oxideav-indeo` rises to
  640 (was 630).
- Indeo 3 (IV31 / IV32) spec/06 §1.2 / §3.3 per-row continuation-byte
  lookahead offset — `indeo3::entropy` gains `RowLookahead` and the
  `MAX_ROW_LOOKAHEAD_OFFSET` constant (`= 4`), completing the §3.3
  variable-byte continuation surface left after round 314's
  `continuation_needed` test. When a literal mode byte's primary-table
  dyad overflows, the continuation byte is read at `[ebp + N]` — a
  fixed *positive* displacement from the bitstream cursor that depends
  on which of the cell's (≤ 4) rows is being emitted: row 0 → `+1`,
  row 1 → `+2`, row 2 → `+3`, row 3 → `+4` (the displacement equals
  `row_index + 1`, one more than the number of `inc ebp` advances the
  earlier rows of the same dyad-pair issued). `RowLookahead::for_row`
  resolves the `(row_index, continuation_offset, read_site_rva)` triple
  for a 0-based row index, returning `None` for rows `>= 4` (no cell
  exceeds four rows, `spec/03 §2.4`). The four §1.2 "cross-row escape
  lookahead" read-site RVAs (`0x10006e18` / `0x10006e91` /
  `0x10006f17` / `0x10006f98`) are transcribed verbatim. 3 new unit
  tests pin the offsets, the read-site RVAs, and the out-of-range row
  rejection; `cargo test -p oxideav-indeo` rises to 630 (was 627).
- Indeo 3 (IV31 / IV32) spec/06 §3.2 mode-byte jump-table per-entry
  dispatch — `indeo3::entropy` gains `JumpTableEntry` and
  `JumpTable::entry(high_nibble)`, resolving each of the two 16-entry
  jump tables' (`0x10006bd4` / `0x10006c50`) slots from the coarse
  round-5 `HighNibbleAction::Other` catch-all into the precise §3.2
  per-(table, high-nibble) outcome: a handler RVA, the fault slot
  (`0x10007a96` → `0x1000854b`, error code 1), or `Unspecified` for the
  second table's `0x5..=0x9` row the spec records as "various" without
  enumerating (left un-invented per the clean-room wall). The
  per-handler RVAs (`0x10006c14` / `0x10006c90` / `0x10006c9c` /
  `0x100072bb` / `0x100072c7` / `0x10007a9b` / `0x1000771c` /
  `0x10007710`) are transcribed verbatim from the §3.2 table.
  `LiteralMode::dispatch_entry()` / `::is_fault()` combine the bit-3
  table selection with the high-nibble index into the single dispatch
  the per-cell unpacker performs; the high-nibble index is masked to
  4 bits so a raw nibble cannot run off the table. 7 new unit tests pin
  both tables entry-by-entry, the shared-vs-divergent slot partition,
  the index masking, the combined dispatch, and the accessor surface;
  `cargo test -p oxideav-indeo` rises to 627 (was 620).
- Indeo 3 (IV31 / IV32) spec/04 §4 VQ_NULL `01` mark-edge executor —
  the `indeo3::cell_null` module gains `mark_edge_cell(buffer,
  geometry)`, the second non-degenerate VQ_NULL arm round 31's
  copy-upper executor deferred. The body at
  `IR32_32.DLL!0x10006a2f..0x10006a55` walks the cell's own pixel
  positions and or-sets bit 7 (`EDGE_MARKER_BIT` = `0x80`) on each,
  marking the cell as an edge / boundary cell (spec/07 §4.2 / §4.4
  sentinel). The executor or-sets bit 7 over each of the cell's
  `row_count` rows × `width_dwords` column groups at the `0xb0`
  per-row stride, preserving the low 7 bits (the marker layers on
  top of the existing pixel content; the spec/07 §4.3 `shl 1` upshift
  discards it downstream). Unlike copy-upper there is no
  upper-neighbour read, so a top-of-strip cell is valid.
  `MarkEdgeGeometry` / `MarkEdgeStats` / typed `MarkEdgeError`
  (zero-width, invalid-row-count, out-of-bounds) mirror the
  copy-upper surface; `VqNullSubCode::is_mark_edge` joins
  `is_copy_upper`. 9 new unit tests; `cargo test -p oxideav-indeo`
  rises to 620 (was 611).
- Indeo 3 (IV31 / IV32) spec/07 §1.4 (cross-ref spec/04 §4) VQ_NULL
  copy-upper executor — the new `indeo3::cell_null` module executes
  the one decode path round 30's `emit_cell_chain` deferred: the only
  path where the predictor row is consumed without a delta add. When
  the binary-tree walker reaches a VQ_NULL leaf whose first two
  sub-code bits are `0`, `0`, the body at
  `IR32_32.DLL!0x100069f4..0x10006a2d` copies the upper-neighbour row
  (`[edi - 0xb0]`) byte-identically into the cell's pixel buffer for
  up to four rows (`[edi]`, `[edi+0xb0]`, `[edi+0x15c]`,
  `[edi+0x20c]`). `copy_upper_cell(buffer, geometry)` runs it over a
  real strip pixel buffer; `COPY_UPPER_RAW_ROW_OFFSETS` pins the four
  §1.4 displacements with `const _` cross-checks that rows 2 / 3 fold
  the body's interleaved `edi += 4` advance into the displacement
  (`0x15c == 2*0xb0 - 4`, `0x20c == 3*0xb0 - 4`). The `VqNullSubCode`
  enum (`VqDataNoIndex` / `CopyUpper` / `MarkEdge`) with a `from_bits`
  decoder surfaces all three spec/04 §4 sub-codes as a typed
  discriminant. Typed `CopyUpperError` covers zero-width,
  invalid-row-count, top-of-strip-source, and out-of-bounds. 12 new
  unit tests; `cargo test -p oxideav-indeo` rises to 611 (was 599).

- Indeo 3 (IV31 / IV32) spec/07 §1.2 + §2.4 (cross-ref spec/06 §6.3
  / §6.4) in-cell predictor chain — the new `indeo3::cell_emit`
  module turns the round-6/7 single-position dyad-pair emission
  (`emit_variant`) into a complete cell decode over a real strip
  pixel buffer. `emit_cell_chain(buffer, geometry, deltas)` walks a
  cell's source rows top to bottom; for each row it reads the
  row-above predictor DWORD out of the buffer (`[edi - 0xb0]`, or the
  §1.3 top-of-strip constant `0x00` when the row-above slot falls in
  the strip's pre-allocated padding), applies the §2.4 left-to-right
  dyad-pair iteration via `emit_variant`, and writes the emitted
  row(s) back so the next row's predictor re-read picks them up —
  reproducing the binary's per-row outer-loop tail at
  `IR32_32.DLL!0x10006fc0..0x10006fdb` plus the §2.1 inner-loop body
  at `0x10006e0f..0x10006e2e`. `rows_per_source_row` pins the
  per-variant destination-pointer advance (variant B advances one
  `0xb0` row stride; variants A / C / D advance two for the vertical
  doubling). The §6.4 sign disposition propagates through: a
  `DyadOutcome::Fault` at any position aborts with
  `CellEmitError::DyadFault { row, dword }` (the binary's error-code-2
  fault at `0x1000855f`). `CellEmitGeometry` carries the cell width
  in dyad-DWORDs, the source-row count, the buffer top-left offset,
  and the `CellVariant`; `DyadDelta` pairs the per-frame-arena primary
  DWORD with the secondary-table word; `CellEmitStats` reports the
  source / emitted row counts and the consumed continuation-byte
  count. Typed `CellEmitError` covers zero-dimension, delta-count
  mismatch, write-out-of-bounds, and the dyad fault. Per the §1
  chapter boundary the module does not read the bitstream (the caller
  supplies the deltas; the codebook-bank values are §3.4 / §7.1
  Extractor territory), does not perform the §1.3 cross-cell
  predictor continuity / §5.5 inter-cell edge fix-up, does not perform
  the §1.4 VQ_NULL copy-upper path, and does not perform the §4.3
  output upshift or §5.7 strip-to-frame assembly. 11 new unit tests
  bring `cargo test -p oxideav-indeo` to 599 (was 588).
- Indeo 3 (IV31 / IV32) spec/05 §5.1 / §5.2 / §7.2 + spec/03 §5.5
  motion-compensation executor — the new `indeo3::mc_exec` module
  lands the first buffer-mutating stage of the MC pipeline.
  `boundary_fixup_dst_cell_offset` runs the §7.2 `[esp+0x34]`
  boundary-fix-up reduction (`bank[+0x700][cl] sar 2 + extra_offset
  + ch`) that the round-15 `mc_address` module deferred, with
  `BOUNDARY_FIXUP_SCRATCH_OFFSET` (`0x34`),
  `BOUNDARY_FIXUP_AUX_SHIFT` (`2`) and
  `advance_boundary_fixup_row` (the spec/07 §1.2 per-row
  `add [esp+0x34], 0xb0`, `BOUNDARY_FIXUP_ROW_ADVANCE`).
  `mc_copy_cell` executes the §5.1 / §5.2 cell copy over a strip
  pixel buffer in the binary's inner-loop order (rows 0+1 read then
  written, rows 2+3 read then written; column groups within a
  4-row band, bands top to bottom) through the round-14 per-DWORD
  kernels, covering all four `McDispatchMode` arms with the §5.2
  half-pel neighbour reads accounted in the safe-Rust bound check
  the binary omits per §4.4; `mc_copy_cell_mv` drives the copy from
  a `PackedMv` (§2.2 mode bits + §2.3 displacement); typed
  `McCopyError` reports the buffer-edge failure modes.
  `apply_per_cell_edge_fixup` executes the spec/03 §5.5 inter-cell
  edge fix-up loop (the spec/07 §1.3 predictor-continuity DWORD
  exchange `[esi+0x24]` → `[edi-4]` / `[edi]` → `[esi+0x28]`, one
  `0xb0` row stride per iteration, do-while `edx -= 4`), with
  `PerCellEdgeFixupError` for its failure modes. 28 new unit tests
  cover the §7.2 reduction, the four copy modes against scalar
  per-byte references, the MV-driven entry, the half-pel-aware
  bound checks, the §5.5 fix-up semantics, and an end-to-end
  fixture run over a spec/02 §7-sized arena from a packed MV to
  actual 8-bit output pixels via the spec/07 §4.3 upshift.

- Indeo 3 (IV31 / IV32) spec/07 §4.3 / §5.6 / §5.7 output-buffer
  write — the new `indeo3::frame_output` module lands the output
  stage the round-27 `frame_exit` §6.2 handoff targets.
  `upshift_7bit_to_8bit` runs the §4.3 1-bit upshift (`shl byte,
  1`) from the internal 7-bit-per-byte representation to 8-bit
  output values, discarding the §4.2 / §4.4 `EDGE_MARKER_BIT`
  sentinel as the spec describes. `OUTPUT_PLANE_ORDER`
  (`[Y, V, U]`) pins the §5.6 step 2 output plane order with a
  `const _` cross-check that it is the exact reverse of the §5.2
  decode-time `PLANE_ITERATION_ORDER` (U → V → Y). `IF09_FOURCC`
  (`0x39304649`, `const _`-checked to spell `"IF09"` in stream
  byte order), `IF09_FOURCC_CASE_RVA` (`0x10004576`) and
  `IF09_PASSTHROUGH_RVA` (`0x1000a53c`) pin the §5.3 / §5.6 IF09 /
  YVU9 passthrough dispatch surface. `assemble_plane_if09` executes
  the §5.7 strip-to-frame assembly: it walks a plane's strips left
  to right, reads each strip's rows from its own
  `FRAME_OUTPUT_SRC_ROW_STRIDE` (`0xb0`) pixel buffer, applies the
  per-byte upshift, and writes the corresponding horizontal slice
  of the caller's full-width output raster, leaving stride padding
  untouched. `strip_min_buffer_bytes` exposes the per-strip walk's
  minimum buffer length; the typed `PlaneAssembleError` enum
  carries the six defensive failure modes (strip-count mismatch,
  width-sum mismatch, width-exceeds-row-stride, short strip
  buffer, narrow output stride, short output buffer). Per the
  chapter boundary the module performs no YUV→RGB conversion
  (§5.4's LUTs are populated by register-indirect stores the audit
  could not pin; §7.2 open question), no §5.5 chroma upsampling
  (IF09 output keeps 4:1:0), and no §6 frame finalisation. 24 new
  unit tests (560 total, was 536).
- Indeo 3 (IV31 / IV32) spec/02 §6.2 per-frame plane-iteration
  terminator + output-reconstruction handoff — the new
  `indeo3::frame_exit` module owns the per-frame layer above the
  round-8 `PlaneDecodeStatus` per-plane classifier. `PLANE_ITERATION_ORDER`
  pins the §8 `[2, 1, 0]` (U, V, Y) count-down loop order (with
  `const _` permutation cross-checks). `PER_PLANE_DECODE_CALL_SITE_RVA`
  (`0x10004637`), `PER_PLANE_DECODE_ENTRY_RVA` (`0x10006538`),
  `PER_PLANE_DECODE_RET_RVA` (`0x10006b94`),
  `PER_PLANE_DECODE_RET_CLEANUP_BYTES` (`0x1c`) and
  `PER_PLANE_DECODE_ARG_COUNT` (`7`) pin the §6 call site, entry,
  and `ret 0x1c` seven-argument cdecl callee stack-cleanup (with a
  `const _` cross-check that `0x1c == 7 * 4`).
  `FRAME_OUTPUT_RECONSTRUCTION_RVA` (`0x10004644`) and
  `FRAME_FAULT_RETURN_RVA` (`0x10006ba2`) pin the §6.2 success
  handoff and the §6 end-of-frame fault path (which returns the §6
  status `3`). `FrameExitDisposition` (`ProceedToReconstruction` /
  `EndOfFrameFault`) carries `proceeds_to_reconstruction()` /
  `is_fault()` / `target_rva()` / `frame_status()`.
  `FramePlaneStatusFold::from_iteration_order` /
  `from_plane_idx_order` fold the three round-8 `PlaneDecodeStatus`
  values into one per-frame disposition, short-circuiting on the
  first faulting plane in §8 iteration order and recording the
  faulting plane's iteration index (`first_fault_iteration_index`)
  and `plane_idx` (`first_fault_plane_idx()`). The module is purely
  dispositional — it does not perform the per-plane binary-tree
  walk (spec/03), does not classify a single plane's `eax` (round-8
  `PlaneDecodeStatus`), does not own the §6.1 per-plane payload byte
  budget (`PlaneByteMap`), and does not perform the output-
  reconstruction stage the §6.2 handoff targets (spec/07). 14 new
  unit tests cover the §8 iteration order + permutation, the §6 RVA
  / cleanup constants, the entry-precedes-ret-and-fault code-memory
  ordering, the reconstruction handoff, both `FrameExitDisposition`
  variants' getters, and the `FramePlaneStatusFold` fold across
  all-ok / first-plane-fault / last-plane-fault / multiple-fault
  short-circuit / plane-idx-order reordering / order-agnostic
  disposition agreement. Total `cargo test -p oxideav-indeo` lib
  count rises to **536 unit tests** (was 522).

- Indeo 3 (IV31 / IV32) spec/02 §9 typed plane-data byte map —
  the new `indeo3::PlaneByteMap` struct + `PictureLayer::plane_byte_map(plane_idx, header, buffer_len) -> Option<PlaneByteMap>`
  expose the §9 "plane-data byte map" diagram as a typed view on a
  present plane. The map carries the §9 landmark offsets as
  absolute byte ranges into the codec-frame input buffer: the
  `num_vectors_range` (§3.1 / §9 row 1, a four-byte u32 range),
  the `mc_vectors_range` (§3.2 / §9 row 2, a `2*num_vectors`-byte
  range — empty on an INTRA plane), the `payload_start` (§3.4 /
  §9 row 3, the first byte of the binary-tree / VQ bitstream
  payload — identical to the owning `PlanePrelude::bitstream_offset`),
  and the §6.1 / §10 item 4 `payload_upper_bound` (the strict
  byte budget the binary-tree decoder may scan, resolved by
  scanning the OTHER present planes for the smallest `plane_base`
  strictly greater than `payload_start` and falling back to
  `buffer_len` when none exists). The `payload_budget()` /
  `prelude_len()` convenience methods expose the §10 item 4
  "end-of-plane padding tolerance" surface and the §3.4 prelude
  length; `plane_byte_map` returns `None` for out-of-range plane
  indices, for `PlanePresence::NullFrame`, and for skipped planes
  (no map exists for either). The upper-bound resolution
  defensively clamps to `payload_start` when a caller passes a
  truncated `buffer_len` so the returned `payload_upper_bound`
  always satisfies `payload_start ≤ payload_upper_bound`. Eight
  new unit tests cover: an INTER Y plane's §9 row-by-row landmarks
  (7 motion vectors, V plane at the next base as the upper bound);
  an INTRA plane's empty `mc_vectors_range` + payload_start
  immediately after the 4-byte u32; the last-plane buffer_len
  fallback; the smallest-following-base selection (against an
  unsorted plane-offset triple); the non-present-plane exclusion
  from the upper-bound scan (a `SkippedNegativeOffset` plane is
  ignored); the NULL-frame + out-of-range plane-index `None`
  paths; the `payload_start`-vs-`PlanePrelude::bitstream_offset`
  cross-check across all three present planes for an INTER frame;
  and the defensive clamp behaviour when `buffer_len <
  payload_start`. The map is purely structural — it does not own
  any payload bytes, does not consult the binary-tree codes, and
  does not alter the existing `PlanePrelude` parse — bridging the
  spec/02 §9 layout to callers that want per-region byte slicing
  (hex dumps, debugger overlays, structural validators) without
  reaching into prelude-size arithmetic themselves.

- Indeo 3 (IV31 / IV32) spec/03 §5.4 end-of-strip edge fix-up
  executor — `indeo3::StripEdgeFixupDims::apply_to_buffer` runs the
  per-row rightmost-column byte duplication
  (`dest[r * 0xb0 + width] = src[r * 0xb0 + width - 1]`) on a
  caller-supplied `&mut [u8]` strip pixel-buffer slice, walking
  `strip_height` rows at the [`STRIP_EDGE_ROW_STRIDE`] (`0xb0`)
  per-row pointer-advance stride. The earlier round surfaced only
  the §5.4 parameter / iteration descriptors and explicitly deferred
  the byte-copy execution to the caller; this round closes that
  contract with a safe-Rust slice executor and a typed
  [`StripEdgeApplyError`] failure surface covering three §5.4
  boundary conditions: `ZeroWidthStrip` (the `mov al, [edi - 1]`
  load lacks a source position), `WidthExceedsRowStride` (the
  `mov [edi], al` write would land on the next row's leading
  cursor, violating §5.2's "visible width sits strictly inside the
  0xb0 allocated stride" invariant), and `BufferTooShort` (the
  slice has fewer bytes than `strip_height × 0xb0`, with both the
  required and supplied byte counts carried for diagnostics). A
  zero-row strip short-circuits to `Ok(0)` without touching the
  buffer (matching the §5.4 spec's `while (rows_remaining)` guard).
  10 new unit tests cover: the zero-row early return; the
  zero-width error; the width-at-stride error; the buffer-too-short
  error with required + supplied counts; the single-row duplication
  (offset 159 → 160 for a luma strip of width 160); the chroma
  walk after the `sar 2` divide (a 240-row stored chroma slot
  walks 60 rows at width 40, with each padding slot at offset 40
  mirroring the rightmost-column byte at offset 39); the
  non-padding-byte preservation invariant (every byte outside the
  per-row write target is left as supplied); the oversize-buffer
  acceptance (only the first `strip_height × 0xb0` bytes are
  touched); the via-`for_slot` luma full-height walk (480 rows);
  and the error-display spec-citation surface (every variant cites
  `spec/03 §5.4`). All offsets, the row stride, the chroma divide,
  and the per-row read/write byte positions trace to
  `03-macroblock-layer.md` §5.4 verbatim.
- Indeo 3 (IV31 / IV32) spec/05 §7.3 reverse-decomposition surface
  — the typed `(x, y, w, h)` recovery from the round-15
  [`indeo3::McCellAddressPair::resolve`] outputs. The new
  `indeo3::cell_geometry` module surfaces
  [`CELL_PIXELS_PER_COLUMN_GROUP`] (`4`) and
  [`CELL_PIXELS_PER_ROW_BAND`] (`4`) — the two §7.3 factors aliased
  to [`MC_COLUMN_GROUP_PIXELS`] / [`MC_BAND_ROWS`] with `const _`
  cross-checks; [`cell_width_from_column_group_count`] /
  [`cell_height_from_row_band_count`] (the §7.3 `cell_w = cl_inner *
  4` / `cell_h = row_band_count * 4` mappings with §2.4 zero-input
  rejection and `u32` overflow guards);
  [`row_band_count_from_ch_register`] (the §7.3 / §7.1 `ecx >> 24`
  upper-byte extraction from the initial `ch` register snapshot);
  [`CellCoords`] / [`cell_coords_from_dst_addr`] (the §7.3 modular
  decomposition `dst_addr → (cell_x = dst_addr mod 0xb0, cell_y =
  (dst_addr - strip_base) / 0xb0)` against [`MC_ROW_STRIDE`]); and
  [`CellRect::from_parts`] / [`reverse_decompose`] — the typed
  shape descriptor + single-call composition of the three sub-
  facets — with a typed [`CellRectDecodeError`] surface for the
  four failure modes (dst-address-below-strip-base, zero column-
  group count, zero row-band count, dimension overflow). Per the
  §7.3 chapter boundary, the module accepts pre-resolved
  `cl_inner` bytes (§7.5 Extractor territory for
  `bank[+0x000][cl]`), leaves strip-pixel-buffer-to-frame
  composition to `spec/07 §5.7`, and leaves visible-width
  classification to [`McPlaneRole::strip_visible_width`]. 34 new
  unit tests cover the two factor constants, the
  `cell_width_from_column_group_count` mapping at typical
  intra-cell + full-strip + chroma-strip widths (with zero-input
  rejection + max-byte arithmetic), the
  `cell_height_from_row_band_count` mapping at typical heights
  (with the same edge cases), the `row_band_count_from_ch_register`
  upper-byte extraction across four bit-patterns, the
  `cell_coords_from_dst_addr` modular decomposition (at strip
  origin, within first row, one row below base, last column of
  strip row, arbitrary strip position + caller-contract violation),
  the `CellRect::from_parts` assembly + per-factor error
  propagation, the `reverse_decompose` end-to-end composition
  including the four-way error fan-out, the cross-module
  consistency identities (`MC_ROW_STRIDE` modulus alignment +
  `MC_COLUMN_GROUP_PIXELS` / `MC_BAND_ROWS` factor equivalence),
  and a forward-reverse round-trip at arbitrary coordinates. Total
  unit-test count rises to 504 (was 470).

- Indeo 3 (IV31 / IV32) spec/02 §6 picture-layer plan → 7-argument
  per-plane decode-call bridge — the typed accessor
  `indeo3::PlaneDecodePlan::to_decode_call()` returning a populated
  `indeo3::PerPlaneDecodeCall` (the §6 7-argument cdecl frame the
  per-plane decoder consumes at `IR32_32.DLL!0x10006538`). The
  bridge keys the §6 codebook-bank discriminant on `plane_idx`
  (luma → `+0x1a00`, chroma → `+0x400`), populates the §6
  constants for the strip-context array view (`+0x300c`) and the
  secondary codebook pointer (`+0x3004`), forwards the plan's §3.4
  `bitstream_offset` as the §6 4th argument, and per spec/02 §10
  item 3 sets `slot_idx_src == slot_idx_dst`. Backed by a new
  sibling constructor
  `indeo3::PerPlaneDecodeCall::for_plane_and_buffer(plane_idx,
  buffer_selector, bitstream_payload_offset)` that takes the
  spec/02 §3.2 / §5.1 buffer-selector bit directly instead of the
  full `FrameFlags`; the existing
  `PerPlaneDecodeCall::for_plane(plane_idx, flags, payload)` keeps
  its signature and delegates to the new constructor (zero
  behavioural change for prior callers). 6 new unit tests cover
  PRIMARY luma, primary V/U chroma (`+0x400` bank), SECONDARY Y
  (slot 0 with luma bank still `+0x1a00` — §6 luma-vs-chroma
  discriminant keys on `plane_idx`, not the buffer bit), bridge-vs-
  `FrameFlags` cross-check across all three planes,
  `for_plane_and_buffer`-vs-`for_plane` equivalence across four
  flag permutations × three plane indices × four payload offsets,
  and out-of-range rejection for the new constructor under both
  buffer-selector polarities. Total unit-test count rises to 470
  (was 464).

- Indeo 3 (IV31 / IV32) spec/02 §4 + §5 + §6 picture-layer →
  strip-context decode-plan bridge — the typed accessor
  `indeo3::PictureLayer::plane_decode_plan(plane_idx, header,
  buffer_selector)` returning an `Option<PlaneDecodePlan>` that
  bundles, for one parsed plane, the §4 `StripGeometry` (plane
  dimensions + per-plane-class strip width + strip count + §4.1
  remainder-formula last-strip width), the §5.1 / §5.2
  `StripSlotDescriptor` (slot index, plane role, per-slot field
  offsets), and the §3.4 bitstream-payload offset + §3.1
  `num_vectors` from the round-2 prelude parser at one typed entry
  point. The new `indeo3::PlaneDecodePlan` struct carries
  `plane_idx`, `buffer_selector`, the `PlaneRole`, `plane_width`
  / `plane_height`, `num_vectors`, `bitstream_offset`,
  `geometry`, `slot_descriptor`, and the `is_luma()` /
  `is_chroma()` / `is_intra()` predicates. The new
  `indeo3::chroma_plane_width(luma_width)` helper surfaces the §4
  picture-decomposition-table `luma_width / 4` chroma subsampling
  (explicitly without the §7 item 4 `& -0x4` mask the chroma
  height helper applies). The accessor returns `None` for any
  `plane_idx ≥ PLANE_COUNT`, for any `PlanePresence::NullFrame` /
  `Skipped*` plane, and applies the §4 picture-decomposition table
  for chroma planes (`(chroma_plane_width(luma_width),
  chroma_plane_height(luma_height))`); for a single-strip plane
  (§4.2 row 1, `W ≤ strip_width`) it writes the §4.1 remainder
  width (`((W-1) mod strip_width) + 1`, = the picture width
  itself) into the slot descriptor's `STRIP_WIDTH` field per §5.2.
  8 new unit tests cover: §4 luma geometry on a 320×240 picture
  (slot 3 primary bank, strip_count 2, aligned), §4 chroma
  subsampled geometry on the V plane of a 320×240 picture (slot 4,
  plane width 80, plane height 60, INTER 2-MV), §4.2 row 1
  single-strip remainder-width path on a 144×112 picture (slot's
  `STRIP_WIDTH` = 144, not the 160 constant), §5.1 secondary-bank
  slot remapping (Y → 0, V → 1, U → 2 when `frame_flags` bit 9 is
  set), `None` for every NULL-frame plane, `None` for a skipped
  plane while sibling planes still return a plan, `None` for
  out-of-range plane indices, and the `chroma_plane_width`
  divide-by-4-without-alignment behaviour on luma widths 0, 4, 16,
  17, 18, 22, 160, 320, 640. Total `cargo test -p oxideav-indeo`
  count rises to **464 unit tests** (was 456).

- Indeo 3 (IV31 / IV32) spec/05 §5.6 MC fetcher → VQ residual
  chapter boundary surface — the typed §5.6 disposition surface
  that pins the MC chapter's terminator and the spec/06 entropy
  chapter's start point. New `indeo3::mc_residual_boundary` module
  surfacing `MC_FETCHER_LAST_WRITE_RVA` (`= 0x1000_6732`, the §5.6
  second-paragraph RVA of the final inner-loop write `mov [edi +
  0x20c], eax`), `MC_FETCHER_LAST_WRITE_DST_OFFSET` (`= 0x20c`, the
  row-3 destination byte offset, equal to `MC_FULL_PEL_ROW_OFFSETS[3]
  = 0x210` minus the §5.1 `lea edi, [edi + 0x4]` mid-loop column
  advance, cross-checked at `const _`-time),
  `MC_CHAPTER_LAST_DST_ROW_INDEX` (`= 3`, the §5.1 band's
  fourth-and-last row index, cross-checked at `const _`-time
  against `MC_BAND_ROWS`), `MC_INNER_LOOP_BAND_ROWS_ALIAS`
  (`= MC_BAND_ROWS as u32`), `VQ_RESIDUAL_DISPATCH_RVA`
  (`= 0x1000_6bac`, the §5.6 first-paragraph + `spec/04 §3.4`
  per-byte unpacker dispatch entry where spec/06 begins, with a
  `const _` cross-check that the RVA strictly follows
  `MC_FETCHER_LAST_WRITE_RVA`), `shares_destination_buffer()`
  (`const`-`true` predicate surfacing the §5.6 first-paragraph
  disposition that the MC prediction and the VQ residual share the
  same destination buffer; no per-cell intermediate copy), the
  `McCellDisposition` enum (`PredictionOnly` /
  `PredictionThenResidual`) classifying the §5.6 first-paragraph
  two-path post-MC chain with `requires_residual()` /
  `residual_application()` typed predicates, the
  `ResidualApplication` enum (`None` / `InPlaceOverPrediction`)
  with `is_none()` / `is_in_place()` predicates, and the
  `McToVqHandoff` composite struct bundling the MC-chapter
  terminator RVA with the spec/06 start RVA at one typed surface
  with `McToVqHandoff::for_disposition(disp)` returning a
  populated handoff for `PredictionThenResidual` (and `None` for
  `PredictionOnly`, the latter case ends the cell at the MC chapter
  terminator without spec/06 dispatch) and `rva_delta()` returning
  the positive byte distance between the two RVAs. 25 new unit
  tests cover the four RVA / offset constants (3: spec-match,
  inner-loop range; 3: spec-match, row-3-minus-LEA; 2: band-rows
  alias, band-height identity; 3: spec-match, strict-after-MC,
  delta-≥-`0x100`), the shared-destination-buffer disposition
  (1), the `McCellDisposition` predicates (3: prediction-only-no-
  residual / prediction-then-residual-yes-residual / residual-
  application-mapping) and variants-distinct (1), the
  `ResidualApplication` predicates (2: none-is-none / in-place-is-
  in-place) and variants-distinct (1), the
  `McToVqHandoff::for_disposition` happy paths (3: prediction-only
  returns `None` / prediction-then-residual returns populated /
  rva-delta-matches-constants), the struct's `Copy` semantics (1),
  the round-trip identity over both dispositions (1), and cross-
  module sanity (3: row-offset-table re-use, mode-agnostic
  terminator across the §2.2 four-way fork, two-path partition of
  the post-MC chain). Per the §5.6 chapter boundary, the module
  deliberately does not perform the MC fetcher's inner-loop reads
  / writes (owned by `mc_kernel`), does not perform the per-byte
  mode read at `IR32_32.DLL!0x10006bac` (owned by the spec/06
  unpacker dispatch in `entropy`), does not perform the VQ
  residual addition itself (spec/06 unpacker territory), does not
  classify a cell-state byte as chained or unchained (`spec/04
  §7.5` territory; this module accepts a pre-classified
  `McCellDisposition` from the caller), and does not own the §5.1
  inner-loop row layout (owned by `MC_FULL_PEL_ROW_OFFSETS`; this
  module re-uses the final entry through
  `MC_FETCHER_LAST_WRITE_DST_OFFSET`). Spec source:
  `docs/video/indeo/indeo3/spec/05-motion-compensation.md` §5.6
  cross-referenced with `spec/04 §3.4` (the unpacker dispatch
  entry), `spec/04 §7.5` (the shared INTER / VQ_DATA leaf-byte
  table), and `spec/05 §5.1` (the MC fetcher inner loop whose
  final write is the chapter terminator). Total `cargo test -p
  oxideav-indeo` count rises to **456 unit tests** (was 431).

- Indeo 3 (IV31 / IV32) spec/05 §5.5 chroma-plane scaling surface —
  the typed §5.5 disposition surface that pins the MC fetcher's
  behaviour on the chroma slot indices `1, 2, 4, 5` relative to the
  luma slot indices `0, 3`. New `indeo3::mc_chroma` module surfacing
  `LUMA_PIXEL_PER_CHROMA_PIXEL` (`= 4`, the §5.5 third-bullet 4:1
  horizontal × 4:1 vertical YVU9 subsampling ratio) with a `const
  _` cross-check against the macroblock-layer `LUMA_STRIP_WIDTH` /
  `CHROMA_STRIP_WIDTH` split (`160 == 40 * 4`),
  `CHROMA_PACKED_MV_FACTOR_IS_BUFFER_STRIDE` (`= true`, the §5.5
  fourth-bullet disposition that the §3.3 packed-MV `176`-factor is
  the buffer-allocated row stride and not a plane-resolution
  constant) with a `const _` cross-check that
  `MV_PIXEL_OFFSET_ROW_STRIDE == MC_ARENA_ROW_STRIDE`,
  `MC_KERNEL_GEOMETRY_IS_PLANE_ROLE_INVARIANT` (`= true`, the §5.5
  first-bullet disposition that the MC fetcher's inner-loop
  geometry constants `MC_BAND_BYTE_STRIDE` / `MC_BAND_ROWS` /
  `MC_BYTES_PER_DWORD` / `MC_INNER_LOOP_BYTES_PER_ITER` /
  `MC_INNER_LOOP_DWORDS_PER_ITER` are not parameterised on plane
  role) re-exported under the long-form alias
  `McKernelGeometryIsPlaneRoleInvariant`,
  `MvPixelOffsetInterpretation::LumaOrChromaUniformBufferStride`
  (the §5.5 fourth-bullet typed-surface enum with a single variant
  pinning the uniform-buffer-stride interpretation) with
  `pixel_offset_row_stride()` returning the §3.3 row-stride factor
  `0xb0`, and `McPlaneRole` (`Luma` / `Chroma`) as a local typed
  surface for the §5.1 slot-index split with
  `from_strip_slot_index(slot) -> Option<McPlaneRole>` (`0, 3` ⇒
  `Luma`; `1, 2, 4, 5` ⇒ `Chroma`; other ⇒ `None`),
  `strip_visible_width()` returning `LUMA_STRIP_WIDTH` /
  `CHROMA_STRIP_WIDTH`, `strip_allocated_row_stride()` returning
  the constant `MC_ARENA_ROW_STRIDE` for both roles (the §5.5
  second bullet "the row stride remains the constant `0xb0`"),
  `cell_size_subsampling_ratio()` (`1` for luma, `4` for chroma),
  `is_luma()` / `is_chroma()` predicates, and `chroma_cell_size(
  luma_width, luma_height) -> Option<(u32, u32)>` const associated
  function that applies the §5.5 third-bullet integer-multiple
  4:1 / 4:1 subsampling (returns `None` for non-multiple inputs).
  30 new unit tests cover the subsampling-ratio constant (2),
  the packed-MV buffer-stride disposition (2), the kernel-geometry
  invariance flag and its constants (2), the `MvPixelOffsetInterpretation`
  enum (2), the slot-index classifier across luma (1), chroma (1),
  out-of-range (1), and the full in-range `0..=5` coverage (1), the
  visible-width getters for luma (1) / chroma (1), the
  allocated-row-stride getters for luma (1) / chroma (1) / cross-
  role equality (1), the cell-size subsampling-ratio getters (2),
  the role predicates (1), the `chroma_cell_size` happy paths (3:
  4×4 / 16×16 / 160×240) and rejections (2: non-multiple width /
  non-multiple height) and zero-edge (1), and cross-module sanity
  (4: chroma both-axis subsampling round-trip, visible-width vs
  luma ratio, row-stride independent of visible width, packed-MV
  interpretation disposition). Per the §5.5 chapter boundary, the
  module deliberately does not perform the codec-init population
  of the codebook-bank `+0x000` / `+0x100` sub-tables with chroma
  cell sizes (host-side per `spec/04 §5.3`), does not perform the
  §5.1 inner-loop reads / writes (owned by `mc_kernel`), does not
  perform the §2.3 source-pointer arithmetic (owned by
  `apply_mv_source_offset`), and does not derive the luma vs
  chroma slot-index split itself beyond the §5.1 cross-reference
  (`strip_context::PlaneRole` owns the strip-context-array
  dimension's split; this module's `McPlaneRole` is the smaller
  §5.5-scoped surface for the MC fetcher only).
- Indeo 3 (IV31 / IV32) spec/05 §4.4 "no explicit boundary check"
  surface — the typed disposition for the absence of a bounds
  check on the §2.3 source-pointer arithmetic. New
  `indeo3::mc_bounds` module surfacing the `MC_NO_BOUNDARY_CHECK`
  `const`-`true` flag (the §4.4 paragraph 1 disposition that the
  parser does not validate the §2.3 `add esi, sign_extend(packed >>
  2)` against the source strip's allocated buffer), the
  `SourcePointerBoundsCheck` enum (`BinaryDoesNotCheck` /
  `CallerOptsIn`) for documentation-time selection of the binary
  vs safe-Rust-opt-in path, the `MvSourceOffsetClass` enum
  (`InRegion` / `OutOfRegion` / `Underflow`) classifying the
  resulting source-pointer byte address against a supplied strip
  region, and the `mv_source_offset_in_strip_region(dst_cell_base,
  mv_offset, strip_region_bytes_total) -> MvSourceOffsetClass`
  const classifier that surfaces the §4.4 paragraph 3 opt-in
  check without consuming the §2.3 arithmetic itself.
  `STRIP_REGION_LUMA_240_BYTES` (`= 0xa500`) pins the §4.4
  paragraph 2 first-bullet worked-example region size (`0xb0 *
  240` for a 240-pixel-tall luma plane) with `const _`
  cross-checks against the §4.1 `strip_region_bytes(240)` formula
  and the §4.4 prose's explicit `0xa500` / decimal `42_240`
  figures. `STRIP_REGION_LUMA_240_FITS_IN_ARENA` (`= false`)
  pins the §4.1-footnote-tracked discrepancy that the §4.4 prose's
  "far smaller than the 0x8020-byte arena's total" claim does
  *not* hold numerically (`0xa500 > 0x8020`), matching round 17
  mc_arena's `StripArenaCapacity::fits_in_arena` disposition.
  `PaddingPixelPreservation` enum (`DeterministicAtCodecInit` /
  `PreservedAcrossFramesByStripEdgeFixup`) carries the §4.4
  paragraph 2 second-bullet "the strip allocator initialises the
  buffer to a deterministic pattern at codec init / the edge
  fix-up loops preserve those padding pixels across frames"
  two-half disposition as a typed surface linking `spec/02 §7`
  codec init to round 11's `StripEdgeFixupDims`. 27 new unit
  tests cover the disposition flag (1), the worked-example
  constants and the arena-discrepancy assertion (3),
  `SourcePointerBoundsCheck` predicates (3), `MvSourceOffsetClass`
  predicates (3), `mv_source_offset_in_strip_region` happy paths
  (3: zero-MV / positive-in-region / negative-in-region),
  out-of-region edges (4: past-end / at-region-end /
  one-past-end / one-under-end), underflow edges (3: -0x200 from
  0x100 / -0x100 from 0x100 in-bounds / -0x101 from 0x100
  underflow), zero-size region (1), saturating add at
  `u64::MAX` (1), `PaddingPixelPreservation` predicates (3), and
  cross-module sanity (2: canonical row stride / mid-region
  zero-MV). Per the §4.4 chapter boundary, the module
  deliberately does not perform the §2.3 source-pointer
  arithmetic itself (owned by `apply_mv_source_offset`), does not
  own the strip allocator or its deterministic-pattern fill
  (host-side per `spec/02 §7`), does not perform the §5.4
  strip-edge fix-up (owned by `StripEdgeFixupDims` /
  `StripEdgeRowIter`), does not range-check `dst_cell_base`
  against the strip region (assumed in-range from the §7.2
  `mc_dest_address` chain), and never indicates a malformed
  stream — per §4.4 the binary "tolerates [out-of-region MVs]
  without faulting; they are not malformed from the decoder's
  perspective".
- Indeo 3 (IV31 / IV32) spec/05 §4.3 source-pointer plumbing —
  the typed §4.3 surface that links round 16's `bank_select`
  resolved `(dst_slot, src_slot)` pair to round 15's `mc_address`
  cell-data DWORD load through the per-plane decoder →
  cell-state dispatcher stack-frame hand-off the §4.3 four-
  instruction fragment at
  `IR32_32.DLL!0x10006638..0x10006641` runs
  (`sub eax, edi; add eax, [esp + 0x54]; mov edx, [esi + 4 * eax];
  mov [esp + 0x24], edx`). New `indeo3::mc_source_plumbing` module
  surfacing the two decoder argument byte-offsets
  `DECODER_ARG_SRC_SLOT_OFFSET` (`= 0x54`, the source-slot-index
  argument written by the §4.2 inversion at
  `IR32_32.DLL!0x100045e9..0x100045fd`) /
  `DECODER_ARG_DST_SLOT_OFFSET` (`= 0x58`, the destination-slot-
  index argument written at
  `IR32_32.DLL!0x100045c3..0x100045d4`), the three cell-state
  dispatcher scratch-slot byte-offsets
  `DISPATCHER_SCRATCH_SRC_DATA_OFFSET` (`= 0x24`, the source
  cell-data DWORD written by the §4.3 fragment's `mov [esp+0x24],
  edx`), `DISPATCHER_SCRATCH_DST_DATA_OFFSET` (`= 0x28`, the
  destination cell-data DWORD) /
  `DISPATCHER_SCRATCH_EXTRA_OFFSET_OFFSET` (`= 0x38`, the §7.2
  `idx_src + 1` companion that the §5.5 boundary fix-up
  consumes), and the element-to-byte index shift
  `STRIP_CTX_ARRAY_ELEMENT_SHIFT` (`= 2`, the §4.3 line 3
  `mov edx, [esi + 4 * eax]`). `const _` cross-checks pin the
  `+ 4` adjacency between the two decoder-arg slots and between
  the two cell-data dispatcher scratch slots. The
  `DecoderStackArg` enum (`SrcSlot` / `DstSlot`) typed-picks one
  of the two decoder arguments with `byte_offset()`, `role()`
  (returning the round-15 `CellAddrRole`), and
  `dispatcher_scratch()` linking it to its companion
  `DispatcherScratch`; the `DispatcherScratch` enum
  (`SrcCellData` / `DstCellData` / `ExtraOffset`) typed-picks one
  of the three scratch slots with `byte_offset()`, `role()`
  (extra-offset carries the source role per the §7.2
  `idx_src + 1` derivation), and `is_source_companion()` (`true`
  only for `ExtraOffset`). `SourcePlumbingPair::for_role` runs
  the §4.3 mapping in one entry point and returns the typed
  `(decoder_arg, dispatcher_scratch)` pair whose two halves share
  the same role. `is_self_copy_degenerate(dst_slot, src_slot)`
  surfaces the §4.3 closing predicate
  (`dst_slot == src_slot` ⇒ self-copy); the §4.3 prose's
  "no such frame is observed in the binary" is cross-validated
  against `McBankAssignment::is_self_copy` on every well-formed
  §4.2 inversion (always `false`, since the §4.2 inversion always
  produces slots `BANK_INVERSION_DELTA = 3` apart). 33 new unit
  tests cover the five offset constants (3 + 3 + 3 distinct-slots
  / adjacency / spec-match), the element-index shift identity
  (1), the two `DecoderStackArg` variants' getter outputs (6),
  the three `DispatcherScratch` variants' getter outputs (7),
  the `SourcePlumbingPair::for_role` round-trip identity over
  both roles (5), the `is_self_copy_degenerate` predicate over
  equal slots / distinct slots / every `McBankAssignment::resolve`
  output / the `McBankAssignment::is_self_copy` agreement (4),
  and the scratch-vs-arg cross-frame disjoint-ranges invariant
  (1). Per the §4 chapter boundary, the module deliberately does
  not perform the cell-data DWORD load itself (owned by
  `mc_address`), does not resolve `(dst_slot, src_slot)` (owned
  by `bank_select`), does not perform the §2.3 source-pointer
  arithmetic (owned by `apply_mv_source_offset`), and does not
  enforce per-strip bounds (per §4.4 the binary itself does not
  either). Spec source:
  `docs/video/indeo/indeo3/spec/05-motion-compensation.md` §4.3
  cross-referenced with `spec/02 §6` table rows 2-3 and `spec/05
  §7.2` for the dispatcher-scratch chain. Total `cargo test -p
  oxideav-indeo` count rises to **374 unit tests** (was 341).

- Indeo 3 (IV31 / IV32) spec/05 §4.1 strip pixel-buffer arena
  geometry — the typed §4.1 surface that links round 8's strip-
  context slot layout (the six base-pointer fields at
  `[ctx+0x00..+0x14]`) to round 15's `mc_address` cell-position
  decoding entry (the per-cell `dst_cell_data` / `src_cell_data`
  DWORDs the MC fetcher consumes). New `indeo3::mc_arena` module
  surfacing `MC_ARENA_LEN` (`= 0x8020`, aliased to the round-8
  `PIXEL_BUFFER_ARENA_LEN` heap-block size from
  `IR32_32.DLL!0x10003cdc..0x10003ce3` with a `const _` cross-
  check), `MC_ARENA_ROW_STRIDE` (`= 0xb0`, the byte stride between
  successive rows of a strip's pixel buffer, `const _`-checked
  against both `mc_kernel::MC_ROW_STRIDE` and
  `reconstruct::PREDICTOR_ROW_STRIDE`), and
  `STRIP_PIXEL_BUFFER_ALIAS_COUNT` (`= 6`, re-exporting the §4.1
  "six aliases of the strip's pixel buffer" identity by its §4.1
  name). The `StripPixelBufferAlias` enum (`Base0` / `Base1` /
  `Base2` / `Base3` / `Base4` / `Base5`) gives a typed pick of
  one of the six aliases with `from_index(0..6) -> Option<Self>`,
  `as_index()`, and `slot_relative_byte_offset()` returning one
  of `slot_field::BASE_PTR_{0..5}` per `spec/02 §5.2`.
  `strip_region_bytes(plane_height_pixels)` runs the §4.1
  worked-example arithmetic `MC_ARENA_ROW_STRIDE *
  plane_height_pixels` in `u64`, and
  `StripArenaCapacity::for_plane_height` pins the §4.1 footnote
  predicate `region_bytes <= MC_ARENA_LEN` (yielding the boundary
  height `MC_ARENA_LEN / MC_ARENA_ROW_STRIDE = 186`, with the
  §4.1 worked-example height 240 flagged as not fitting —
  surfacing the arithmetic discrepancy the §4.1 prose mentions
  between the arena size and the per-strip region size).
  `base_pointer_aliases_equal` encodes the §4.1 / `spec/03 §5.2`
  "six pointers are aliases of the same per-strip region"
  invariant as a `slot_bytes: &[u8] -> Option<bool>` over the
  six little-endian DWORDs at the slot-relative offsets,
  returning `None` if the slice does not extend through the last
  base-pointer field. The module deliberately does not perform
  the heap allocation itself (the `IR32_32.DLL!0x10003cdc` call
  is host `LocalAlloc` territory), does not enforce per-strip
  bounds at MC-fetcher time (§4.4 the binary itself does not
  range-check the §2.3 source-pointer arithmetic), does not own
  or populate the slot's six base-pointer fields (codec-init at
  `IR32_32.DLL!0x10003edc..0x10003f3a` writes them), does not
  perform the §4.2 ping-pong bank pick or the §4.3 source /
  destination slot inversion (owned by `bank_select`), and does
  not own the arena's per-frame contents (those are written by
  `mc_kernel` and `reconstruct`). 21 new unit tests cover the
  §4.1 arena-geometry constants (3), the alias enum's round-trip
  indexing and out-of-range rejection (4), the alias byte offsets
  against `slot_field::BASE_PTR_*` and the 4-byte-apart DWORD-
  alignment invariant (3), the boundary-with-slot-stride identity
  (1), the `strip_region_bytes` worked example / zero-height /
  no-wrap-on-u32-MAX cases (3), the `StripArenaCapacity`
  boundary-height arithmetic and the §4.1 worked-example "does
  not fit" case (4), the `base_pointer_aliases_equal` well-
  formed / malformed / short-slice / boundary-slice cases (4),
  and inter-module row-stride cross-checks linking `mc_arena` to
  `mc_packed::MV_PIXEL_OFFSET_ROW_STRIDE` and
  `cell_subarray::PER_CELL_EDGE_ROW_STRIDE` (2). The new module
  is re-exported as `indeo3::StripPixelBufferAlias`,
  `indeo3::StripArenaCapacity`, `indeo3::strip_region_bytes`,
  `indeo3::base_pointer_aliases_equal`, `indeo3::MC_ARENA_LEN`,
  `indeo3::MC_ARENA_ROW_STRIDE`, and
  `indeo3::STRIP_PIXEL_BUFFER_ALIAS_COUNT`.

- Indeo 3 (IV31 / IV32) spec/05 §4.2 ping-pong bank selection — the
  `frame_flags` bit 9 source / destination slot inversion the
  per-plane decoder builds at
  `IR32_32.DLL!0x100045b1..0x100045fd` before pushing the
  `[esp+0x54]` / `[esp+0x58]` arguments to the binary-tree walker.
  New `indeo3::bank_select` module surfacing `BANK_INVERSION_DELTA`
  (`= 3`, the §4.2 "plane_idx + 3" identity aliased to
  `PRIMARY_BANK_SLOTS[i] - SECONDARY_BANK_SLOTS[i]` and
  cross-checked per plane), the `Bank` enum (`Primary` / `Secondary`)
  with `Bank::from_buffer_selector` decoding `frame_flags` bit 9 via
  the typed `FrameFlags::buffer_selector()` accessor (matching the
  parser's `test ch, 0x2` on the `frame_flags` high byte at
  `IR32_32.DLL!0x100045b1`), `Bank::opposite()` (involution,
  Primary ⇔ Secondary), `Bank::slot_for_plane(plane_idx)` (with the
  `plane_idx >= PLANE_COUNT` guard matching
  `strip_slot_index`), and `Bank::is_primary()` /
  `Bank::is_secondary()` predicates. `McBankAssignment::resolve(flags,
  plane_idx)` runs the §4.2 mapping in one entry point and returns
  the resolved `(dst_slot, src_slot, dst_bank)` triple with the
  source bank wired to `dst_bank.opposite()`. `McBankAssignment::src_bank()`,
  `is_self_copy()` (always `false` for a well-formed result; the
  §4.2 "never observed in the binary" same-bank degenerate case),
  and `slot_delta()` (`abs_diff` of the two slot indices, identically
  `BANK_INVERSION_DELTA` for any `resolve()` result) round out the
  surface. Per §4.2 the destination is the bank the *current* frame
  writes into and the source is the bank the *previous* frame wrote
  into — i.e. the MC "previous frame" reference; the two slot
  indices differ by exactly `BANK_INVERSION_DELTA` and the
  ping-pong invariant holds between consecutive frames whose bit 9
  flips (frame N's `dst_slot` is frame N+1's `src_slot`). The
  module deliberately does not perform the strip-context-slot read
  (that's `mc_address::CellSubarrayIndex`), does not load the
  per-cell sub-array DWORDs (those are populated by the spec/03 §6
  open-question-4 pre-frame cell-stack setup), and does not own the
  per-frame bank-state machine that flips bit 9 across frames (the
  encoder owns that sequence; the decoder just consults the
  per-frame value). 28 new unit tests cover `BANK_INVERSION_DELTA`
  cross-checks per plane (4), the `Bank` constructor against the
  §4.2 bit-9 / parser convention including the "other bits
  irrelevant" rule (3), `Bank::opposite` involution (2),
  `Bank::is_primary` / `is_secondary` partitioning (1),
  `Bank::slot_for_plane` against the spec/02 §5.1 tables for both
  banks across all three planes plus the out-of-range plane_idx
  guard (3), the resolved `(dst, src)` triple for each of the six
  legal `(bit-9, plane)` combinations (6), the `is_self_copy()` /
  `slot_delta()` invariants across all combinations (3), agreement
  with the round-8 `strip_slot_index` for both the destination and
  the (inverted) source halves (2), the source-bank-is-dst-bank-
  opposite identity across all combinations (1), the rejection of
  out-of-range `plane_idx` at the resolver (1), and the ping-pong
  two-frame identity (frame N's `dst` becomes frame N+1's `src`
  when bit 9 flips, both for slots and for banks) across all
  planes (2). The new module is re-exported as `indeo3::Bank`,
  `indeo3::McBankAssignment`, and `indeo3::BANK_INVERSION_DELTA`.

- Indeo 3 (IV31 / IV32) spec/05 §5.4 / §7.2 cell-position decoding
  entry — the cell-state dispatcher's index-arithmetic chain that
  resolves the per-cell destination and source pixel-buffer
  addresses the round-14 MC fetcher's inner loop consumes. New
  `indeo3::mc_address` module surfacing the §7.2 / §4.3
  `shl eax, 0x4` at `IR32_32.DLL!0x10006615` as `CELL_SLOT_STRIDE`
  (`16`) and the §7.2 "cell-slot index 0..15" upper bound as
  `CELL_SLOT_INDEX_MAX` (`15`). `CellSlotBase::from_bank_byte`
  applies the post-`shl 0x4` step to the raw `bank[+0x200][ch]`
  one-byte lookup, returning the cell-slot base index; the
  `is_within_meaningful_range()` predicate flags the §7.2 in-bound
  vs out-of-bound ranges without rejecting (per §7.5 the table
  values themselves are Extractor territory). `CellSubarrayIndex::dst`
  / `CellSubarrayIndex::src` compose
  `idx_dst = 16 * cell_slot + dst_slot` /
  `idx_src = 16 * cell_slot + src_slot` (the §7.2 / §4.3 per-cell
  sub-array element indices loaded at
  `IR32_32.DLL!0x10006638..0x10006641`), with `byte_offset()`
  returning the post-shift `mov edx, [esi + 4 * eax]` byte offset.
  `CellAddrEntry::dst(cell_data_ptr)` /
  `CellAddrEntry::src(cell_data_ptr, extra_offset)` hold the
  destination / source cell-data DWORDs tagged with their
  `CellAddrRole` (`Dest` / `Src`) and carry the §7.2 `[esp+0x38]`
  extra-offset companion (loaded from `strip_ctx_arr[idx_src + 1]`,
  used by the §5.5 boundary fix-up) on the source-role branch.
  `mc_dest_address(dst_entry, cell_pos_aux)` composes the §5.4 /
  §7.2 `dst_addr = dst_cell_data + bank[+0x700][cl]` step
  (`usize::checked_add` for safe-Rust wrap detection — per §4.4
  the binary itself does not bounds-check). `mc_source_address(src_entry,
  cell_pos_aux, packed_mv)` composes the §5.4 / §7.2
  `src_addr = src_cell_data + bank[+0x700][cl] + sign_extend(packed_MV >> 2)`
  chain, threading the §2.3 / §3.4 `apply_mv_source_offset`
  sign-extending MV displacement. `McCellAddressPair::resolve`
  runs the complete §7.2 chain in one entry point, returning the
  (dst, src) byte-address pair the MC fetcher's inner loop
  consumes; `McAddressError` enumerates the four safe-Rust check
  failures (`DestAddressOverflow`, `SrcAddressOverflow`,
  `SrcMvDisplacementInvalid`, `RoleMismatch`). The `is_self_copy()`
  predicate flags the §8.2 item 8 identity-MV degenerate case
  (`dst_slot == src_slot` + `packed_mv == 0` →
  `dst_addr == src_addr`). Per the §5.4 / §7 chapter boundary, the
  module deliberately does not own the `bank[+0x200]` slot-index
  LUT or the `bank[+0x700]` cell-position aux LUT (per-entry values
  are §7.5 Extractor territory), does not own the strip-context
  per-cell sub-array DWORDs (pre-frame cell-stack setup is spec/03
  §6 open question 4), does not perform the §7.2 `[esp+0x34]`
  boundary-fix-up reduction (composite of `bank[+0x700][cl] sar 2 +
  extra_offset + ch` — feeds §5.5 not the MC fetcher), does not
  perform the §7.3 `(x, y, w, h)` reverse decomposition, and does
  not perform the §4.2 `frame_flags` bit 9 source / destination
  slot inversion (a per-plane-decoder decision). 29 new unit tests
  cover the §7.2 / §4.3 cell-slot-stride constants (3), the
  `CellSlotBase` shape including the §7.2 in-range / out-of-range
  predicate at the byte boundary (4), the `CellSubarrayIndex`
  composition including the §4.2 ping-pong `dst_slot - src_slot`
  delta and the §7.2 byte-offset = element × 4 cross-check (4),
  the `CellAddrEntry` role-tagged shape (2), the
  `mc_dest_address` / `mc_source_address` composition covering
  identity-MV / positive / negative displacements and `usize`
  wrap / signed underflow rejections (7), the complete
  `McCellAddressPair::resolve` chain including swapped-role
  rejection and `McAddressError` propagation for all three
  arithmetic failure modes plus the §8.2 item 8 self-copy degenerate
  case (8), and a `CELL_STACK_ENTRY_SIZE` cross-module consistency
  check linking the new module's `byte_offset()` to the existing
  `cell_subarray` 4-byte-per-entry constant (1).

- Indeo 3 (IV31 / IV32) spec/05 §5.1 / §5.2 / §5.3
  motion-compensation cell-copy inner-loop kernel. New
  `indeo3::mc_kernel` module surfacing the §5.1 full-pel inner-loop
  shape (`MC_ROW_STRIDE` = `0xb0`,
  `MC_INNER_LOOP_DWORDS_PER_ITER` = 4,
  `MC_INNER_LOOP_BYTES_PER_ITER` = 16, `MC_BAND_ROWS` = 4,
  `MC_BAND_BYTE_STRIDE` = `0x2c0`, `MC_COLUMN_GROUP_PIXELS` = 4)
  and the four hard-coded source-byte offsets at
  `IR32_32.DLL!0x1000670d..0x1000673d`
  (`MC_FULL_PEL_ROW_OFFSETS` = `[0, 0xb0, 0x160, 0x210]`,
  `mc_full_pel_row_dword`, `McKernelStep::for_row`).
  `McKernelGeometry::new(width_px, height_px)` enforces the §5.1
  multiple-of-4 width/height invariants and the §5.3 row-stride
  bound (`MC_MAX_CELL_WIDTH_BYTES` = `0xb0`).
  The §5.2 per-DWORD averaging kernels: `mc_vert_half_pel_pair`
  for the `01` path (`(src[i] + src[i + 0xb0]) >> 1` via the
  shared `average_7bit` SWAR identity,
  `MC_VERT_HALF_PEL_NEIGHBOUR_OFFSET` = `0xb0`),
  `mc_horiz_half_pel_pair` for the `10` path
  (`(src[i] + src[i + 1]) >> 1` with the in-DWORD byte splice
  `(src_dword >> 8) | (src_dword_next << 24)`,
  `MC_HORIZ_HALF_PEL_NEIGHBOUR_OFFSET` = `1`), and
  `mc_both_half_pel_quad` for the `11` path (the §2.2 / §5.2 2×2
  unweighted box filter, composed horizontal-pair-first /
  vertical-pair-second). All three kernels share the same
  `(a + b) >> 1` byte-parallel identity used by
  `reconstruct::average_7bit`, confirming the §2.2 "no separate
  filter coefficient tables" disposition. The new
  `McKernelStep::outer_band_advance()` (`0x2c0`) and
  `McKernelStep::inner_column_group_advance()` (`4`) helpers
  surface the inner-loop / outer-loop pointer advances per §5.1.
  Per the §5 chapter boundary the module deliberately does not
  own the strip pixel-buffer arena, does not slice-bounds-check
  source pointers (per §4.4 the binary itself does not), does not
  address the §5.6 VQ-residual-after-MC chain, and does not
  validate the §5.4 cell-position decode against the `0xf423f`
  sanity sentinel (that check lives in `cell_loop`'s
  `CELL_POSITION_MAX` per §3.3). 31 new unit tests cover the §5.1
  / §5.3 constants and immediates (8), the §5.1 / §5.3
  `McKernelGeometry::new` invariants including zero / odd-width /
  odd-height / over-stride rejections (8), the §5.1 row-offset
  helper and step-tuple surface (5), the §5.2 averaging kernel
  correctness across vertical / horizontal / both-half-pel paths
  including byte-parallel no-bleed verification and rounding
  semantics (9), and the inter-module row-stride consistency
  check linking the kernel to `reconstruct::PREDICTOR_ROW_STRIDE`
  and `mc_packed::MV_PIXEL_OFFSET_ROW_STRIDE` (1).

- Indeo 3 (IV31 / IV32) spec/05 §2.2 / §2.3 / §3.3 / §3.4 packed-MV
  bit-layout decode and four-way MC dispatch. New
  `indeo3::mc_packed` module surfacing the §3.4 packed-MV byte
  layout (`bits 31..2 = pixel_offset`, `bit 1 = horiz half-pel`,
  `bit 0 = vert half-pel`): `PackedMv::from_raw` wraps the DWORD,
  `PackedMv::pixel_offset` recovers the signed strip-pixel byte
  offset via the §2.3 / §3.4 `sar edx, 0x2` at
  `IR32_32.DLL!0x100066f3` (`MV_PIXEL_OFFSET_SHIFT` = 2),
  `PackedMv::mode` returns the §2.2 four-way `McDispatchMode`
  (`FullPel` / `VerticalHalfPel` / `HorizontalHalfPel` /
  `BothHalfPel`) by inspecting `MV_MODE_BITS_MASK` (0x3) with each
  variant carrying its inner-loop RVA (`0x1000670d` / `0x10006780`
  / `0x1000684b` / `0x100068f8`).
  `apply_mv_source_offset(dst_cell_base, offset)` /
  `PackedMv::source_address` model the §2.3
  `src_addr = dst_cell_base + sign_extend(packed_MV >> 2)`,
  returning `None` on signed underflow as a safe-Rust safety net
  (per §4.4 the binary itself performs no bounds check).
  `pack_mv_components(vert, horiz, vert_lsb, horiz_lsb)` is the
  constructive inverse — the §3.3 closing-arithmetic write
  `((176*vert + horiz) << 2) | (horiz_lsb << 1) | vert_lsb`. The
  §3.3 row-stride constant `MV_PIXEL_OFFSET_ROW_STRIDE` (176 / 0xb0)
  is aliased to `PREDICTOR_ROW_STRIDE` with a `const _`
  cross-check. 20 new unit tests cover the §3.4 mode-bit
  disjointness and shift width (3), the §2.2 four-way dispatch
  including bits-outside-mask invariance and inner-loop-RVA
  uniqueness (7), the §2.3 sign-extending source-pointer arithmetic
  including signed underflow (4), and the `pack_mv_components`
  round-trip across representative `(vert, horiz)` and all four
  mode-bit pairs (6). Per the §3 / §5 chapter boundary, this round
  lands the decode + dispatch surface only — not the §5.1 / §5.2 /
  §5.3 cell copy (per-row byte-pair averaging filter, `0xb0`-stride
  destination walk), not the `(vert, horiz)` re-decomposition (the
  dispatcher uses the combined offset directly per §2.3), and not
  the bounds-check against the strip-buffer arena (per §4.4 the
  binary has no such check).

- Indeo 3 (IV31 / IV32) spec/05 §1 per-plane packed-MV table layout
  and INTER-leaf indexing surface. New `indeo3::mc_table` module
  surfacing the 1 KiB arena at `inner_instance[0x000..0x3ff]` the
  picture-layer parser writes (`MV_TABLE_BASE_OFFSET` = `0x000`,
  `MV_TABLE_ENTRY_SIZE` = `4`, `MV_TABLE_BYTES` = `0x400`,
  `MV_TABLE_MAX_BYTE_INDEXABLE_ENTRIES` = `256`,
  `MV_INDEX_SCALE_SHIFT` = `2`). `MvTableParserArm::from_frame_flags`
  resolves the §1.2 four-way parser-arm dispatch on `frame_flags`
  bits 4 + 5 (`MV_HALFPEL_HORIZ` = `0x10`, `MV_HALFPEL_VERT` = `0x20`,
  combined `MV_HALFPEL_MASK` = `0x30`), with each arm carrying its
  `[ecx + 4*edx]` write-site RVA — `IR32_32.DLL!0x10004572`
  (full-pel), `0x10004493` (horizontal half-pel only), `0x10004510`
  (vertical half-pel only), `0x10004426` (both half-pel) — and its
  per-component half-pel `<<= 1` disposition
  (`applies_half_pel_horizontal` / `applies_half_pel_vertical`).
  `mv_table_entry_byte_offset` enforces the 256-entry bound and
  returns the table-byte offset of entry `i` via
  `MV_TABLE_BASE_OFFSET + i * MV_TABLE_ENTRY_SIZE`.
  `MvIndexFetch::for_index` composes the §1.3 INTER-leaf sequence
  (`xor eax,eax; mov al,[ebp]; shl eax,0x2; add eax,inner_instance`
  at `IR32_32.DLL!0x100065f2..0x10006607`) into a single descriptor
  carrying `(index, table_byte_offset, parser_arm, validity)` — up
  to but not including the `mov eax, [eax]` table dereference.
  `MvIndexValidity::classify` enumerates the §1.4 read-side
  disposition of an MV-index byte against the plane's `num_vectors`
  count: `WrittenThisFrame` (`index < num_vectors`, the only
  well-formed disposition), `StaleTailEntry`
  (`num_vectors <= index < 256`, residual prior-frame content),
  `OutOfRange` (`index >= 256`, unreachable from the one-byte
  index path). 27 new unit tests cover the arena-layout constants
  (5), the four-way parser-arm dispatch including the
  bits-outside-mask invariance and write-site RVA uniqueness (7),
  the per-entry byte-offset helper across the full 256-entry range
  (4), the §1.4 validity classifier across all three branches plus
  the `num_vectors > 256` corner case (6), and the
  `MvIndexFetch::for_index` descriptor's helper-agreement and
  parser-arm-tracking integration (5). Per the §1 chapter boundary,
  this round lands the table-layout / index-arithmetic surface
  only — not the packed-MV bit-layout decode (§3 — bottom 2 bits
  filter mode, upper 30 bits signed strip-pixel byte offset), not
  the four-way MC fetcher dispatch (§5.1 / §5.2 / §5.3), and not
  the half-pel byte-pair averaging filter (§5.2).

- Indeo 3 (IV31 / IV32) spec/03 §5.4 strip-edge fix-up parameter
  surface. New `indeo3::strip_edge` module surfacing the
  end-of-strip rightmost-column-duplication fix-up's per-slot
  dimensions and per-row iteration. `StripEdgeFixupDims::for_slot`
  reads the destination slot's `+0x18` strip-height and `+0x1c`
  strip-width fields and applies the per-plane-role disposition the
  binary's branch at `IR32_32.DLL!0x10006b5e..0x10006b61` selects:
  luma slots 0/3 preserve the fields verbatim, chroma slots 1/2/4/5
  apply `sar 2` (`STRIP_EDGE_CHROMA_SHIFT` = 2, the 4:1 chroma
  subsampling ratio from `spec/02 §4.1`), and scratch slots 6..31
  yield `None` so callers can detect a non-dispatchable slot.
  `StripEdgeRowIter` walks the strip's full height yielding one
  `StripEdgeRow` per row (with `row_cursor_byte_offset` at the
  `0xb0`-stride row start and the §5.4 `mov al, [edi-1]; mov [edi],
  al` read/write offsets `(-1, 0)`). `STRIP_EDGE_ROW_STRIDE` (`0xb0`)
  reuses the same per-row stride as `PER_CELL_EDGE_ROW_STRIDE` (the
  strip's allocated row stride). `STRIP_EDGE_BYTE_READ_OFFSET` (`-1`)
  / `STRIP_EDGE_BYTE_WRITE_OFFSET` (`0`) are surfaced as constants
  alongside the `strip_edge_byte_copy_offsets()` accessor returning
  the `(-1, 0)` pair. 17 new unit tests cover the chroma-shift /
  row-stride / byte-copy-offset constants (4), `StripEdgeFixupDims`'s
  luma-preserve / chroma-divide / scratch-rejection branches plus
  remainder-strip widths and `sar` truncation (6), and
  `StripEdgeRowIter`'s zero-height / single-row / multi-row /
  size-hint / `ExactSizeIterator` behaviour plus a per-slot
  chroma-vs-luma row-count integration check (7). Per the §5
  chapter boundary, this round lands the parameter / iteration
  surface only — not the pixel-buffer byte copy itself (the
  one-line `dest[i] = src[i - 1]` lives in any caller's pixel-buffer
  view), not the `+0x18` / `+0x1c` field byte-loads from the
  strip-context slot (callers pass the values already-loaded), and
  not the pre-frame pixel-buffer allocation (spec/02 §10).

- Indeo 3 (IV31 / IV32) per-cell sub-array wiring (`spec/03` §5.1 /
  §5.3 / §5.5). New `indeo3::cell_subarray` module surfacing the
  read-only indexing arithmetic for the cell-stack at each strip-
  context slot's `[+0x40..]` byte range. `cell_stack_slot_offset` /
  `cell_stack_array_offset` enforce the §5 derived bound
  (`CELL_STACK_MAX_ENTRIES` = `(0x400 - 0x40) / 4 = 240`) and return
  the byte offset of entry `(slot_idx, entry_idx)` via the
  `slot_idx * STRIP_SLOT_STRIDE + 0x40 + 4 * entry_idx` formula that
  the binary's `[ecx + 4*ebx + 0x40]` indexing implements.
  `CellStackReadSite` enumerates the three §5.3 read sites within
  `IR32_32.DLL!0x10006538` (`SourceSlotTop` at `0x1000656c`,
  `DestSlotTop` at `0x10006ab5`, `CellPositionProbe` at
  `0x10006651`) with `zero_means_strip_edge`,
  `zero_means_mirror_bank`, and `uses_dest_slot_base` predicates
  matching the binary's per-site `entry == 0` dispositions.
  `CellStackTopDispatch::from_dest_slot_top` classifies the
  destination-slot stack-top DWORD into the §5.4 `StripEdgeFixup`
  branch (zero → strip-edge fix-up at `0x10006b4b..0x10006b80`) or
  the §5.5 `InterCellFixup { cell_data_ptr }` branch (non-zero →
  per-cell edge fix-up at `0x10006574..0x100065a3`, carrying the
  cell-data pointer through). The §5.5 per-cell edge fix-up's
  pixel-buffer-side byte-offset constants — `[esi + 0x24]` read site
  (`PER_CELL_EDGE_PREV_BR_OFFSET`), `[esi + 0x28]` write site
  (`PER_CELL_EDGE_PREV_BR_NEXT_OFFSET`), the row stride `0xb0`
  (`PER_CELL_EDGE_ROW_STRIDE`), and the per-iteration `edx -= 4`
  height step (`PER_CELL_EDGE_HEIGHT_STEP`) — are surfaced as
  named constants. 21 new unit tests cover the entry-size /
  begin-offset / max-entries constants (3), the §5.5 byte-offset
  constants (3), the slot-relative and array-absolute offset
  helpers' happy paths, bounds rejection, and within-stride
  invariant (6), the three read-site predicates (3), and the
  cell-stack-top dispatch's zero / non-zero / minimum-non-zero /
  maximum-non-zero classification (4) and pointer accessor. Per the
  §5 boundary, this round does not pre-populate cell-stack entries
  (the pre-frame mechanism is `spec/03` §6 open question 4), does
  not run the per-cell edge fix-up byte loop (the pixel-buffer
  DWORD shuffles still need allocated strip buffers per `spec/02`
  §10), and does not decode entry contents (the 4-byte cell-data
  pointer interpretation lives with the pre-population routine).

- Indeo 3 (IV31 / IV32) outer per-cell row/column loop preamble
  (`spec/04` §3.3). New `indeo3::cell_loop` module bridging round 7's
  `emit_variant` per-position kernel to round 8's strip-context slot
  geometry, encoding the binary's `IR32_32.DLL!0x1000665e..0x10006670`
  four-step sequence as a structured outcome.
  `dispatch_cell_preamble(bank, cell_stack_top, cl_in, ecx_in)` runs
  the preamble in one call: picks the `CodebookBankView`
  (`from_cell_stack_top` → `Primary` for any non-zero stack top, the
  `+0xb00` `Mirror` view for the intra-context-without-stack first
  cell of a strip's MC_TREE walk per §3.3 step 1, the
  `cmp [esi + 4*eax + 0x40], 0` fork at `0x1000665e`), loads the
  cell-position offset DWORD from `bank[+0x300 + 4*cl]`
  (`CELL_DATA_TABLE`), runs the `cmp esi, 0xf423f` (= `999_999`,
  `CELL_POSITION_MAX`) sanity check (any `>=` → `CellPositionFault`
  matching the `0x10006b97` malformed-bitstream fault), reads the new
  `cl` row counter from `bank[+0x000 + cl]` (`CL_ROW_COUNTER_LUT`),
  and clears the intra-context flag (`INTRA_CONTEXT_CLEAR_MASK` =
  `0xbfffffff`, the complement of `INTRA_CONTEXT_FLAG` = bit 30) so
  the returned `CellLoopState` (with `cl_inner`,
  `cell_position_offset`, `bank_view`, and `ecx_post_clear`) is the
  exact handoff the §3.4 VQ_DATA / VQ_NULL fork
  (`test ecx, 0x800000`, exposed as `CellLoopState::vq_data_flag`)
  consults. `advance_row(cl_before, edi_before, cell_column_step)`
  steps the row counter and the `edi` write cursor exactly as the
  variant kernels' `dec cl` / `[esp+0x20]` advance does, returning a
  `CellRowAdvance` whose `end_of_column` flag fires on the
  `cl_after == 0` transition; `iterate_column_rows(cl_inner,
  edi_start, cell_column_step)` materialises the full per-column
  `(cl, edi)` walk an inner variant kernel call sequence visits.
  Bank-layout constants (`CELL_BANK_LEN` = `0x1300` = 4.75 KB total,
  `CL_ROW_COUNTER_LUT` = `0x000`, `CH_CONTROL_LUT` = `0x100`,
  `SLOT_INDEX_LUT` = `0x200`, `CELL_DATA_TABLE` = `0x300`,
  `CELL_POSITION_TABLE` = `0x700`, `MIRROR_TABLE_OFFSET` = `0xb00`)
  surface the §1.1 sub-table table for direct caller access, and the
  bank's primary-sub-table sizes (3 × 256 byte LUTs + 2 × 1 KiB DWORD
  tables = `0xb00`) are cross-checked against `MIRROR_TABLE_OFFSET`
  at compile time. Lower-level lookup primitives
  `read_cl_row_counter(bank, cl)` and
  `read_cell_position_dword(bank, cl)` are surfaced for callers that
  want bank reads without the full preamble. 19 new unit tests cover
  the bank-layout constants vs the §1.1 table, the sanity-check
  constants (`0xf423f` and the `INTRA_CONTEXT_FLAG` complement), the
  `CodebookBankView::from_cell_stack_top` zero-vs-non-zero fork
  (including `0x1869f` the strip-slot sentinel and `u32::MAX`), the
  `read_cl_row_counter` byte-table lookup with short-slice rejection,
  the `read_cell_position_dword` little-endian DWORD load,
  `dispatch_cell_preamble`'s mirror-vs-primary view selection, the
  intra-context-flag clear (bit 30 cleared, bit 31 and bits 0..29
  preserved), the `0xf423f` cell-position-offset boundary (`>=` is
  fault, `==` is fault, `-1` passes), the `vq_data_flag` bit-31 read,
  `advance_row`'s mid-column row-stride step vs end-of-column
  column-step transition (the variant-dependent `[esp+0x20]` value),
  the zero-counter caller-bug rejection, `iterate_column_rows` for
  4-row and 8-row cell columns (the §2.2 4×4 / 8×8 cell sizes) plus
  single-row and empty-column degenerates, and a round-trip from
  `dispatch_cell_preamble`'s state through `iterate_column_rows` to
  the per-row `(cl, edi)` sequence the inner variant kernel would
  call against. Per the §3.3 boundary, this round lands the
  preamble's structural surface only — not the per-byte unpacker
  dispatch at `0x10006bac` (the high-nibble jump table is
  `spec/06`'s subject), not the inner column-advance per-row store
  (`spec/07` §2.2's variant shapes were round 7), not the strip
  pixel-buffer allocation (the strip-context array's byte buffer is
  still future work per `spec/02` §10), and not the static
  cell-geometry-bank entry values (Extractor territory per
  `spec/04` §7.1). Spec source:
  `docs/video/indeo/indeo3/spec/04-vq-codebooks.md` §3.3 (with the
  fault disposition cross-referenced to `spec/05` §5).

- Indeo 3 (IV31 / IV32) strip-context array + per-plane decode-call
  signature (`spec/02` §4–§7). New `indeo3::strip_context` module
  landing the per-codec-frame picture-decomposition state that sits
  between the round-2 prelude consumer and the round-3 binary-tree
  walker. `StripGeometry::for_luma(plane_width, plane_height)` /
  `::for_chroma` resolve a plane's strip count + per-strip widths from
  `(plane_width, plane_height)` using the `ceil(W / strip_width)` and
  `((W-1) mod strip_width) + 1` formulae the parser at
  `IR32_32.DLL!0x10003d6b` / `0x10003f53` implements;
  `strip_slot_index(plane_idx, buffer_selector)` + `StripSlotDescriptor`
  surface the §5.1 dispatchable-slot indexing (primary bank slots 3..5,
  secondary bank slots 0..2, plane-role classification slots 0/3 =
  luma, slots 1/2/4/5 = chroma); `PerPlaneDecodeCall::for_plane(
  plane_idx, flags, bitstream_payload_offset)` encodes the §6
  seven-argument cdecl frame the picture-layer parser hands the
  per-plane decoder (`IR32_32.DLL!0x10006538`) with the codebook-bank
  discriminant resolved (`+0x1a00` for luma at
  `IR32_32.DLL!0x100045a3..0x100045a9`, `+0x400` for chroma at
  `0x1000458d..0x10004593`); `PlaneDecodeStatus::from_eax` classifies
  the integer status code (`0` → `Ok`, `3` → `Malformed`, any other
  non-zero → `Malformed`); the codec-init §7 strip-count helpers
  `luma_strip_slot_count` (= `ceil(width / 160)`) /
  `chroma_strip_slot_count` (= `ceil(luma_width / 16)`) +
  `chroma_plane_height` (= `(luma_height / 4) & -4`) record the
  per-`ICDecompressBegin` arithmetic the future codec-init code will
  consume. The per-slot field layout (`+0x00..+0x14` six base pointers,
  `+0x18` strip height, `+0x1c` strip width, `+0x20..+0x3f` strip
  scratch, `+0x40+` per-cell sub-array) is surfaced as the
  `slot_field` constants submodule. Strip-context array constants
  (`STRIP_SLOT_STRIDE` = 0x400, `STRIP_SLOT_COUNT` = 32,
  `DISPATCHABLE_SLOT_COUNT` = 6, `STRIP_SLOT_SENTINEL` = 0x1869f,
  `STRIP_ARRAY_OFFSET_IN_INSTANCE` = 0x414, `INSTANCE_STATE_LEN` =
  0x3010, `PIXEL_BUFFER_ARENA_LEN` = 0x8020,
  `INSTANCE_STRIP_ARRAY_VIEW_PTR` = 0x300c,
  `INSTANCE_SECONDARY_CODEBOOK_PTR` = 0x3004,
  `INSTANCE_LUMA_CODEBOOK_BANK` = 0x1a00,
  `INSTANCE_CHROMA_CODEBOOK_BANK` = 0x400) and the primary /
  secondary slot-bank lookup tables (`PRIMARY_BANK_SLOTS` = [3, 4, 5],
  `SECONDARY_BANK_SLOTS` = [0, 1, 2]) are surfaced as constants. 25
  new unit tests cover the slot-index discipline (both banks; out-of-
  range rejection), plane-role classification (all six dispatchable
  slots + scratch range), slot descriptor offsets, the slot-field
  offset table, the strip-geometry aligned + remainder formulae per
  the §4.2 informative table (W ≤ 160 / 161..320 / 321..480 /
  481..640) for both luma and chroma plane widths, the strip-widths
  iterator, the per-plane-decode-call argument frame for luma /
  chroma × primary / secondary (4 combinations) with the
  codebook-bank discriminant + the §10 item 3 src == dst invariant,
  the `eax` status classification, the codec-init strip-count
  arithmetic, and the parser-formula helpers (ceil-div + last-strip-
  width). Per the spec/02 §10 boundary this round lands the
  structural surface only — not the byte buffer of the strip-context
  array, not the binary-tree walker's writes into the sub-array
  (spec/03's subject), not the motion-compensation reads from the
  pixel buffer (spec/05), and not the §5.2 sub-array field semantics
  beyond `+0x1c`. Spec source:
  `docs/video/indeo/indeo3/spec/02-picture-layer.md`.

- Indeo 3 (IV31 / IV32) four cell-shape variant inner-loop emission
  kernels (`spec/07` §2.2 / `spec/04` §2.2). `emit_variant(variant,
  predictor, primary_delta, secondary_word)` runs the shared
  `apply_dyad_pair` add and then applies the per-variant store shape
  the codebook DWORD's two mode bits select: variant A
  (`CellVariant::Plain`, `IR32_32.DLL!0x1000670d`) stores the
  dyad-pair DWORD directly to two adjacent rows (vertical doubling,
  no saturation); variant B (`CellVariant::WithEdge`,
  `0x10006780`) writes one row of the per-byte average of the
  predictor and the dyad result with the `0x7f7f7f7f` 7-bit clamp;
  variant C (`CellVariant::DoubledRow`, `0x1000684b`) writes that
  average to two rows; variant D (`CellVariant::FullyDoubled`,
  `0x100068f8`) writes the `and 0xfefefefe; shr 1` per-byte halve to
  two rows. Results are returned as a `VariantEmission { outcome,
  rows }` where `rows` (a fixed-capacity `RowEmission`) lists the
  output DWORD(s) to store at successive `0xb0`-stride row offsets;
  a `DyadOutcome::Fault` emits zero rows. `average_7bit(a, b)`
  (the `(a & b) + (((a ^ b) >> 1) & 0x7f7f7f7f)` SWAR average) and
  `halve_fefefefe(value)` (`(value & 0xfefefefe) >> 1`) expose the
  two per-byte arithmetic primitives, alongside the `CLAMP_7BIT_MASK`
  (`0x7f7f7f7f`) and `HALVE_CARRY_MASK` (`0xfefefefe`) constants.
  10 new unit tests cover the two masks, the per-byte floor average
  (no inter-byte carry bleed) + its bit-7 clamp, the per-byte halve
  (no cross-byte bleed), each variant's row shape (plain two-row,
  with-edge one-row, doubled-row two-row, fully-doubled two-row),
  the fault → zero-rows path across all four variants, and the
  continuation-outcome propagation. Per the spec/07 boundary this
  round lands the per-position variant store shape only — not the
  outer per-cell row/column loop (the `cl` / `ch` counter walk,
  spec/04 §3.3), the strip-buffer assembly, the 7→8-bit upshift, or
  the YUV→RGB / IF09 conversion (§5), and not motion compensation
  (`spec/05`). Spec source:
  `docs/video/indeo/indeo3/spec/07-output-reconstruction.md`.

## [0.0.1](https://github.com/OxideAV/oxideav-indeo/releases/tag/v0.0.1) - 2026-05-24

### Other

- spec/07 output-reconstruction kernel (predictor + softSIMD dyad add)
- round 5 — byte-level entropy (spec/06)
- round 4 — VQ codebook materialisation (spec/04)
- CHANGELOG — correct round-3 test count to 15
- round 3 — macroblock-layer binary-tree walk (spec/03)
- round 2 — picture-layer plane-prelude parser
- round 1 — frame-header + bitstream-header parser
- Round 0 — clean-room rebuild scaffold (orphan master)

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
