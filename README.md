# oxideav-indeo

Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Round 2 — Indeo 3 (IV31 / IV32) picture-layer plane preludes.**
Round 1 landed the 64-byte combined header parser
([`FrameHeader::parse`], `spec/01`). Round 2 adds
[`PictureLayer::parse`], a structural decoder for the per-plane
preludes (`num_vectors` + `mc_vectors[]`) that immediately follow
each plane offset, per
`docs/video/indeo/indeo3/spec/02-picture-layer.md`.

`PictureLayer::parse` honours every spec/02 §2/§3 rule that
governs the prelude bytes:

- Plane iteration order U → V → Y (§2 count-down `plane_idx ∈
  {2, 1, 0}`).
- Plane skip rules — `plane_offset < 0` (interpreted as i32) and
  `plane_offset > data_size/8` — both surface as
  [`PlanePresence`] variants distinct from the parsed prelude.
- `num_vectors` u32 LE (§3.1) and `mc_vectors[num_vectors]` two-
  signed-byte array (§3.2) per plane.
- Half-pel arithmetic right shift on the vertical / horizontal
  components driven by `frame_flags` bits 4 and 5 (§3.3), with
  the shifted-out LSB preserved as the half-pel sub-field used
  by the §3.3 packed-MV formula.
- Precomputed `bitstream_offset` (`plane_base + 4 + 2*num_vectors`,
  §3.4) for the spec/03 macroblock-layer hand-off.
- NULL-frame plane-iteration skip (§1, `data_size == 0x80`).

No binary-tree / VQ payload decode yet — that work sits on
`spec/03-macroblock-layer.md`. Indeo 2 / 4 / 5 still have only a
multimedia.cx wiki snapshot under `docs/video/indeo/indeoN/wiki/`,
no `spec/` chapters, so they remain at the round-0 scaffold
pending docs work.

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
| spec/02 §4 plane → strip → cell → block   | deferred |
| spec/02 §5 strip-context array            | deferred |
| spec/02 §6 per-plane decode call          | deferred |
| spec/03 macroblock layer                  | deferred |

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
* Constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `FLAG_YVU9_8BIT`, `NULL_FRAME_DATA_SIZE_BITS`, `MIN_DIMENSION`,
  `MAX_WIDTH`, `MAX_HEIGHT`, `PLANE_COUNT`, `PLANE_IDX_U`,
  `PLANE_IDX_V`, `PLANE_IDX_Y`, `NUM_VECTORS_FIELD_LEN`,
  `MC_VECTOR_ENTRY_LEN`, `MIN_PRELUDE_LEN`.
* `alt_quant_indices(byte) -> (primary, secondary)` for §3.9.

## License

MIT.
