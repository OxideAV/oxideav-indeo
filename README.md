# oxideav-indeo

Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Round 5 — Indeo 3 (IV31 / IV32) byte-level entropy.**
Round 1 landed the 64-byte combined header parser
([`FrameHeader::parse`], `spec/01`). Round 2 added
[`PictureLayer::parse`], the per-plane prelude decoder (`spec/02`).
Round 3 added [`decode_plane_tree`], the binary-tree walk over a
plane's bitstream payload (`spec/03`), returning a typed
[`CellTree`] of INTRA / INTER leaf cells whose VQ sub-tree leaves
carry the raw codebook-index byte. Round 4 adds the `indeo3::vq`
module (`spec/04`), which materialises the codebook resources those
indices reference and resolves a packed codebook entry into the
structural pieces the per-cell unpacker consumes:

- [`DyadDeltaTable`] — the static 8 KB dyad-mode delta table
  (`.data + 0x1003d088`, 16 banks × 512 B), indexed
  `(high_nibble << 9) + col` per the dyad handler, surfacing the
  audit-noted bank-15 row restriction.
- [`CodebookEntry::decode`] — the packed codebook DWORD: two mode
  bits select one of four [`CellVariant`]s; bits 2..31 are a signed
  (`sar 2`) byte offset into the per-frame arena.
- [`seed_dispatch_entries`] — the static codebook seed-dispatch
  table (`.data + 0x1003ed4c`, 129 byte-pairs) packed as
  `((al << 8) + bl) << 9` with signed source bytes.
- [`VqArena`] + [`VqArena::apply_alt_quant`] — the per-frame arena
  and the `alt_quant[]` band-selection overlay (`cb_offset << 11`
  bias applied once, then per active band a primary copy at
  stride 128 and a secondary copy at stride 2048).
- [`VqNullRuntime`] — the runtime VQ_NULL sub-codes (copy-upper /
  mark-boundary / unpacker-dispatch).

Round 5 adds the `indeo3::entropy` module (`spec/06`), the
byte-level entropy surface that consumes round 4's VQ codebook
state. spec/06 §1 establishes that Indeo 3 has exactly four
bitstream mechanisms and that there is **no Huffman / arithmetic
coder and no fixed VLC longer than the 2-bit binary-tree code**;
the first three were already modelled (spec/03 §2 tree codes,
spec/03 §3.4 / spec/04 §3.1 leaf-byte indices, spec/04 §4 VQ_NULL
prefix code). Round 5 lands the fourth — the per-cell mode-byte
stream:

- [`ModeByte::classify`] — the §2.3 / §3.1 mode-byte split: bytes
  `0x00..=0xF7` are literal dyad indices ([`LiteralMode`], with the
  high-nibble jump-table selector, low-nibble × 2048 arena-band
  base, and low-nibble bit 3 [`JumpTable`] flavour); bytes
  `0xF8..=0xFF` are RLE escapes ([`RleEscape`]).
- [`continuation_needed`] — the §3.3 variable-byte rule: the dyad
  sum's sign bit decides whether a continuation byte is read
  (making each literal cost 1 or 2 bytes), with
  [`apply_continuation_xor`] modelling the `xor eax, 0x80008000`
  back-out.
- [`RleEscape::accepted_at`] — the §4.3 per-position acceptance
  matrix ([`PositionClass`]): `0xFB`/`0xFC`/`0xFD` accepted
  everywhere, `0xFE`/`0xFF` at row-starts, `0xF8`/`0xF9`/`0xFA`
  cell-start-only, narrowing across continuations.
- [`fb_category_table`] + [`FbCounter`] — the §4.4 `0xFB`
  counter-byte category lookup (built from the spec's normative
  seed ranges: `0x01..=0x1F` → copy, `0x21..=0x3F` → mark-skipped,
  rest → zero) and the counter decomposition (`(counter & 0x1F) +
  1` cells, bit 5 copy/skip disposition).

Per the spec/06 §8 boundary, round 5 stops at the entropy
question — *which* bytes the stream consumes and *how* each is
classified. The pixel emission (the `add eax, [esi + 4*edx +
0x400]` chain, the `0x7f7f7f7f` mask, the dyad → pixel writes) is
`spec/07`; [`DyadAddress`] computes only the dyad entry's *address*
from the mode byte's nibbles, not its value.

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

Round 5's `indeo3::entropy` module resolves the per-byte
mode-byte stream and the `0xF8..=0xFF` RLE escapes round 4
deferred to `spec/06`. What remains is the pixel emission itself
(the dyad-pair → pixel-pair expansion and the predictor arithmetic,
`spec/07`) plus motion compensation (`spec/05`); neither is started
yet. Indeo 2 / 4 / 5 still have
only a multimedia.cx wiki snapshot under
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
| spec/03 §4.2 codebook-bank lookup tables  | structure (spec/04) |
| spec/03 §5 strip-context pixel layout     | deferred (spec/07) |
| spec/04 §1.3 static dyad delta table (8 KB) | yes (`DyadDeltaTable`) |
| spec/04 §2.1 packed codebook DWORD format | yes (`CodebookEntry`) |
| spec/04 §2.3 dyad table `(hi<<9)+col` index | yes |
| spec/04 §4 VQ_NULL runtime sub-codes      | yes (`VqNullRuntime`) |
| spec/04 §5.1 static seed-dispatch table   | yes (`seed_dispatch_entries`) |
| spec/04 §6 `alt_quant[]` per-frame overlay | yes (`VqArena`) |
| spec/04 §1.2 arena `0x8020` vs `0x8800`   | DOCS-GAP (self-contradictory) |
| spec/04 §5.2 per-frame seed-block build   | deferred (Extractor §7.1) |
| spec/06 §1 entropy-surface inventory (4 mechanisms) | yes (constants + types) |
| spec/06 §2.3 / §3.1 mode-byte nibble split | yes (`ModeByte` / `LiteralMode`) |
| spec/06 §3.2 two 16-entry jump tables     | selector (`JumpTable`) |
| spec/06 §3.3 variable-byte continuation   | yes (`continuation_needed`) |
| spec/06 §3.4 four cell-unpacker variants  | RVA map (`variant_entry_rva`) |
| spec/06 §4.1 / §4.2 eight RLE escapes     | yes (`RleEscape`) |
| spec/06 §4.3 per-position acceptance matrix | yes (`RleEscape::accepted_at`) |
| spec/06 §4.4 `0xFB` counter-byte category table | yes (`fb_category_table`, `FbCounter`) |
| spec/06 §3 dyad-pair address (`+0x400` / `+0x402`) | yes (`DyadAddress`) |
| spec/06 §5 / §7 pixel emission (dyad → pixel) | deferred (spec/07) |

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
* `oxideav_indeo::indeo3::DyadDeltaTable` — the static 8 KB
  dyad-mode delta table; `::load()`, `::delta(high_nibble, col)`,
  `::bank_base()`, `::as_bytes()` (spec/04 §1.3 / §2.3).
* `CodebookEntry::decode(u32)` + `CellVariant` — packed
  codebook-DWORD decode (spec/04 §2.1).
* `seed_dispatch_entries() -> Vec<SeedEntry>` — static
  seed-dispatch table build (spec/04 §5.1).
* `VqArena` (`::new()`, `::apply_alt_quant(seed, &alt_quant,
  cb_offset)`, `::band_primary_offset()`,
  `::band_secondary_offset()`, `::as_bytes()`) + `VqError` —
  per-frame arena + `alt_quant[]` overlay (spec/04 §1.2 / §6).
* `VqNullRuntime::classify(first_bit, second_bit)` — VQ_NULL
  runtime sub-codes (spec/04 §4).
* `oxideav_indeo::indeo3::ModeByte::classify(u8)` — the spec/06
  §2.3 / §3.1 per-cell mode-byte classifier (`ModeByteKind` ->
  `Literal(LiteralMode)` / `Escape(RleEscape)`); `is_literal()` /
  `is_escape()`.
* `LiteralMode` (`::from_byte`, `high_nibble` / `low_nibble` /
  `jump_table_offset` / `arena_band_offset` / `low_nibble_bit3`,
  `::jump_table()`) + `JumpTable` (`First` / `Second`,
  `::base_rva()`) + `HighNibbleAction::from_high_nibble` — the
  §3.1 / §3.2 nibble dispatch.
* `RleEscape` (`F8..Ff`, `::from_byte`, `::byte()`,
  `::extra_bytes()`, `::accepted_at(PositionClass)`) +
  `PositionClass` (`CellFirst` / `RowFirst` / `Continuation1..3`,
  `::variant_a_row0_base_rva()`) — the §4 RLE escapes + §4.3
  per-position acceptance matrix.
* `continuation_needed(u32)` / `apply_continuation_xor(u32)` — the
  §3.3 variable-byte continuation test + back-out XOR.
* `DyadAddress::new(LiteralMode, col)` — the §3.2 dyad-pair
  primary / secondary offsets within the arena band.
* `fb_category_table() -> [u8; 256]` / `fb_category(u8)` /
  `FbCategory` (`Zero` / `Copy` / `MarkSkipped`, `::value()`,
  `::handler_rva()`) / `FbCounter::decode(u8)` — the §4.4 `0xFB`
  counter-byte category lookup + decomposition.
* `variant_entry_rva(CellVariant)` — the §3.4 per-variant unpacker
  entry RVA.
* Entropy constants: `LITERAL_MODE_MAX`, `RLE_ESCAPE_MIN`,
  `ARENA_BAND_STRIDE`, `PRIMARY_TABLE_DISP`, `SECONDARY_TABLE_DISP`,
  `CONTINUATION_XOR`, `VARIANT_A_ENTRY`..`VARIANT_D_ENTRY`.
* VQ constants: `DYAD_TABLE_LEN`, `DYAD_BANK_COUNT`,
  `DYAD_BANK_STRIDE`, `DYAD_BANK15_VALID_ROWS`, `ARENA_LEN`,
  `ARENA_BANDS_OFFSET`, `ARENA_BAND_COUNT`, `ARENA_BAND_LEN`,
  `ARENA_HALF_LEN`, `PRIMARY_STRIDE`, `SECONDARY_STRIDE`,
  `SEED_TABLE_LEN`, `SEED_PAIR_COUNT`.
* Constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `FLAG_YVU9_8BIT`, `NULL_FRAME_DATA_SIZE_BITS`, `MIN_DIMENSION`,
  `MAX_WIDTH`, `MAX_HEIGHT`, `PLANE_COUNT`, `PLANE_IDX_U`,
  `PLANE_IDX_V`, `PLANE_IDX_Y`, `NUM_VECTORS_FIELD_LEN`,
  `MC_VECTOR_ENTRY_LEN`, `MIN_PRELUDE_LEN`.
* `alt_quant_indices(byte) -> (primary, secondary)` for §3.9.

## License

MIT.
