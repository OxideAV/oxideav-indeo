# oxideav-indeo

Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Round 3 — Indeo 3 (IV31 / IV32) macroblock-layer binary tree.**
Round 1 landed the 64-byte combined header parser
([`FrameHeader::parse`], `spec/01`). Round 2 added
[`PictureLayer::parse`], the per-plane prelude decoder (`spec/02`).
Round 3 adds [`decode_plane_tree`], the binary-tree walk over a
plane's bitstream payload (the bytes that begin at the
`bitstream_offset` round 2 computed), per
`docs/video/indeo/indeo3/spec/03-macroblock-layer.md`. It returns
a typed [`CellTree`] of INTRA / INTER leaf cells; INTRA cells
carry their VQ sub-tree leaves inline.

`decode_plane_tree` honours every spec/03 tree-walk rule:

- The §2.1 MSB-first sentinel-bit reader, modelled with the
  original decoder's two-cursor scheme (the bit buffer drains the
  current byte while the shared `ebp` cursor supplies leaf bytes
  from the next un-loaded byte, per §6 item 7).
- The §2.2 four 2-bit node codes (`00` H_SPLIT, `01` V_SPLIT,
  `10` INTRA/VQ_NULL leaf, `11` INTER/VQ_DATA leaf).
- The §3 MC_TREE walk over a plane-sized root cell (§3.1) with
  H_SPLIT halving height top-first and V_SPLIT halving width
  left-first (§3.2).
- The §3.3 INTRA → VQ_TREE transition on the same physical cell,
  and the §3.4 INTER one-byte MV-index read.
- The §4 VQ_TREE walk: the §4.1 VQ_NULL leaf plus its additional
  2-bit sub-code (`00` copy, `01` skip, `10`/`11` fault), and the
  §4.1 VQ_DATA one-byte codebook-index read.

Per the spec/03 §7 chapter boundary the walk stops at the
per-leaf index-byte fetch: `Cell::Inter` records the raw MV-index
byte and `VqLeaf::Data` the raw codebook-index byte. No VQ
codebook materialisation (`spec/04`), motion compensation
(`spec/05`), or pixel reconstruction (`spec/07`) yet. Indeo 2 / 4
/ 5 still have only a multimedia.cx wiki snapshot under
`docs/video/indeo/indeoN/wiki/`, no `spec/` chapters, so they
remain at the round-0 scaffold pending docs work.

The previous (pre-orphan) implementation was retired alongside the
docs audit dated 2026-05-06 (see
[`AUDIT-2026-05-06.md`](https://github.com/OxideAV/docs/blob/master/AUDIT-2026-05-06.md));
the prior history is preserved on the `old` branch for archival
but is forbidden input for the rebuild.

## Quick start

```rust
use oxideav_indeo::indeo3::{FrameHeader, PictureLayer};

let frame: &[u8] = /* full Indeo 3 codec frame */;
let header = FrameHeader::parse(frame)?;

if header.bitstream.is_null_frame() {
    // sync frame: reproduce output from prior-frame state
    // PictureLayer::parse returns a layer with every plane
    // marked PlanePresence::NullFrame.
} else {
    let layer = PictureLayer::parse(&header, frame)?;
    for (plane_idx, presence) in layer.iter_in_decode_order() {
        if let Some(prelude) = presence.as_prelude() {
            // prelude.motion_vectors carries one MotionVector per
            // mc_vectors[] entry; prelude.bitstream_offset is the
            // absolute index where the spec/03 macroblock layer
            // begins for this plane.
            let _ = (plane_idx, prelude);
        }
    }
}
```

## Spec coverage

| Spec section                              | Covered |
| ----------------------------------------- | ------- |
| §2 frame header (16 B)                    | yes     |
| §2.1 `FRMH` checksum validation           | yes     |
| §2.2 `frame_size > 16` bound              | yes     |
| §3 bitstream header (48 B)                | yes     |
| §3.1 `dec_version == 0x0020`              | yes     |
| §3.2 `frame_flags` named bits             | yes     |
| §3.3 `data_size` + NULL-frame sentinel    | yes     |
| §3.4 signed `cb_offset`                   | surfaced |
| §3.5 bitstream `checksum` (read-only)     | surfaced |
| §3.6 `height` / `width` envelope          | surfaced |
| §3.7 Y / V / U plane offsets              | surfaced |
| §3.9 `alt_quant[16]` byte table + split   | yes     |
| §4 plane-decoder entry                    | deferred |
| §5 byte map                               | covered by tests |
| spec/02 §1 NULL-frame plane-skip          | yes      |
| spec/02 §2 plane iteration order U→V→Y    | yes      |
| spec/02 §2 plane-offset skip rules        | yes      |
| spec/02 §3.1 `num_vectors` u32            | yes      |
| spec/02 §3.2 `mc_vectors[]` two signed bytes | yes   |
| spec/02 §3.3 half-pel arithmetic shift    | yes      |
| spec/02 §3.3 packed-MV formula            | helper   |
| spec/02 §3.4 prelude size + bitstream_offset | yes   |
| spec/02 §4 plane → strip → cell → block   | tree-level (geometry) |
| spec/02 §5 strip-context array            | deferred |
| spec/02 §6 per-plane decode call          | deferred |
| spec/03 §2.1 MSB-first sentinel bit reader | yes     |
| spec/03 §2.2 four 2-bit node codes        | yes      |
| spec/03 §3 MC_TREE walk + halving (§3.1/3.2) | yes   |
| spec/03 §3.3 INTRA → VQ_TREE transition   | yes      |
| spec/03 §3.4 INTER MV-index byte          | raw byte |
| spec/03 §4.1 VQ_NULL leaf + sub-codes     | yes      |
| spec/03 §4.1 VQ_DATA codebook-index byte  | raw byte |
| spec/03 §4.2 codebook-bank lookup tables  | deferred (spec/04) |
| spec/03 §5 strip-context pixel layout     | deferred (spec/07) |

"Surfaced" means the field is exposed verbatim on the typed
struct; the reference decoder does not validate the value, so we
do not either. "Deferred" means the work depends on later spec
chapters that aren't yet in `docs/`.

## Public API

* `oxideav_indeo::indeo3::FrameHeader::parse(&[u8])` — combined
  header decoder.
* `FrameHeaderPreamble`, `BitstreamHeader`, `FrameFlags`,
  `HeaderError`.
* `oxideav_indeo::indeo3::PictureLayer::parse(&FrameHeader, &[u8])`
  — per-plane prelude decoder (spec/02).
* `PictureLayer`, `PlanePresence`, `PlanePrelude`, `MotionVector`,
  `PictureLayerError`.
* `PictureLayer::iter_in_decode_order()`, `::y()`, `::v()`, `::u()`.
* `MotionVector::packed_mv()` — spec/02 §3.3 packing formula.
* `oxideav_indeo::indeo3::decode_plane_tree(&[u8], &PlanePrelude,
  plane_width, plane_height, is_chroma, FrameFlags)` — per-plane
  binary-tree walk (spec/03) returning a `CellTree`.
* `CellTree`, `Cell` (`Inter` / `Intra`), `VqCell`, `VqLeaf`
  (`Null` / `Data`), `VqNull` (`Copy` / `Skip`), `NodeCode`,
  `MacroblockError`. `Cell::geometry()`, `CellTree::cell_count()`.
* Strip-width constants `LUMA_STRIP_WIDTH` (160) /
  `CHROMA_STRIP_WIDTH` (40) (spec/02 §4.1).
* Constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `FLAG_YVU9_8BIT`, `NULL_FRAME_DATA_SIZE_BITS`, `MIN_DIMENSION`,
  `MAX_WIDTH`, `MAX_HEIGHT`, `PLANE_COUNT`, `PLANE_IDX_U`,
  `PLANE_IDX_V`, `PLANE_IDX_Y`, `NUM_VECTORS_FIELD_LEN`,
  `MC_VECTOR_ENTRY_LEN`, `MIN_PRELUDE_LEN`.
* `alt_quant_indices(byte) -> (primary, secondary)` for §3.9.

## License

MIT.
