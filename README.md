# oxideav-indeo

Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Round 11 — Indeo 3 (IV31 / IV32) end-of-strip edge fix-up
parameter surface (`spec/03` §5.4).**
Round 11 adds the `indeo3::strip_edge` module, the parameter /
iteration surface for the §5.4 strip-edge fix-up that runs after a
strip's last cell has been emitted. The fix-up duplicates the
rightmost column of pixels (`mov al, [edi-1]; mov [edi], al`,
byte-by-byte) down the strip's full height; the per-row
pointer-advance step is `0xb0`
([`STRIP_EDGE_ROW_STRIDE`], shared with the §5.5 per-cell stride).
[`StripEdgeFixupDims::for_slot`] reads the destination slot's
`+0x18` strip-height and `+0x1c` strip-width fields and applies
the per-plane-role disposition the binary's
`IR32_32.DLL!0x10006b5e..0x10006b61` branch selects: luma slots
0/3 preserve the fields verbatim, chroma slots 1/2/4/5 apply
`sar 2` ([`STRIP_EDGE_CHROMA_SHIFT`] = 2, the 4:1 chroma
subsampling ratio from `spec/02 §4.1`), scratch slots 6..31 yield
`None` so callers can detect a non-dispatchable slot.
[`StripEdgeRowIter`] walks the (chroma-adjusted) height, yielding
one [`StripEdgeRow`] per row with `row_cursor_byte_offset` at the
`0xb0`-stride row start and the `(-1, 0)` read/write byte-offset
pair ([`STRIP_EDGE_BYTE_READ_OFFSET`] /
[`STRIP_EDGE_BYTE_WRITE_OFFSET`]). Per the §5 chapter boundary,
round 11 lands the parameter / iteration surface only — not the
pixel-buffer byte copy itself (the one-line `dest[i] = src[i - 1]`
lives in any caller's pixel-buffer view), not the `+0x18` / `+0x1c`
field byte-loads from the strip-context slot (callers pass the
values already-loaded), and not the pre-frame pixel-buffer
allocation (`spec/02` §10).

**Round 10 — Indeo 3 (IV31 / IV32) per-cell sub-array wiring
(`spec/03` §5.1 / §5.3 / §5.5).**
Round 10 adds the `indeo3::cell_subarray` module, the read-only
indexing arithmetic for the cell-stack stored inside each strip-
context slot at `[+0x40..]`. [`cell_stack_slot_offset`] /
[`cell_stack_array_offset`] enforce the §5 bound
([`CELL_STACK_MAX_ENTRIES`] = `(0x400 - 0x40) / 4 = 240`) and
return the byte offset of entry `(slot_idx, entry_idx)` via the
`slot_idx * 0x400 + 0x40 + 4 * entry_idx` formula the binary
implements with `[ecx + 4*ebx + 0x40]`. [`CellStackReadSite`]
enumerates the three §5.3 read sites within
`IR32_32.DLL!0x10006538` (`SourceSlotTop` at `0x1000656c`,
`DestSlotTop` at `0x10006ab5`, `CellPositionProbe` at
`0x10006651`) with their two zero-disposition flags
(`zero_means_strip_edge`, `zero_means_mirror_bank`).
[`CellStackTopDispatch::from_dest_slot_top`] classifies the
destination-slot stack-top DWORD into the §5.4 strip-edge fix-up
branch (zero) or the §5.5 inter-cell fix-up branch (non-zero,
carrying the cell-data pointer through). The §5.5 per-cell edge
fix-up byte-offset constants — `[esi + 0x24]` read site
([`PER_CELL_EDGE_PREV_BR_OFFSET`]), `[esi + 0x28]` write site
([`PER_CELL_EDGE_PREV_BR_NEXT_OFFSET`]), row stride `0xb0`
([`PER_CELL_EDGE_ROW_STRIDE`]), and per-iteration height step `4`
([`PER_CELL_EDGE_HEIGHT_STEP`]) — are surfaced as constants for
the future pixel-buffer-side loop. Per the §5 boundary, round 10
lands the indexing surface only — not the cell-stack pre-frame
population (§6 open question 4), not the per-cell edge fix-up
byte loop itself (the pixel-buffer DWORD shuffles run by the
allocated strip buffers, which are still future work per
`spec/02` §10), and not the cell-stack entry-content semantics
(the 4-byte cell-data pointer interpretation lives with the
pre-population routine).

**Round 9 — Indeo 3 (IV31 / IV32) outer per-cell row/column loop
preamble (`spec/04` §3.3).**
Round 9 adds the `indeo3::cell_loop` module, bridging round 7's
per-position `emit_variant` kernel to round 8's strip-context slot
geometry. [`dispatch_cell_preamble`] reproduces the binary's
`IR32_32.DLL!0x1000665e..0x10006670` four-step sequence: pick the
[`CodebookBankView`] (primary vs `+0xb00` mirror) from the cell-stack
top, load the cell-position DWORD from `bank[+0x300 + 4*cl]` with the
`0xf423f` ([`CELL_POSITION_MAX`]) sanity check, read the new `cl`
row counter from `bank[+0x000 + cl]`, and clear the intra-context
flag (`ecx &= 0xbfffffff`). The resulting [`CellLoopState`] carries
the row counter, the cell-position offset, the bank-view choice, and
the post-clear `ecx` for the §3.4 VQ_DATA / VQ_NULL fork
([`CellLoopState::vq_data_flag`]). [`advance_row`] /
[`iterate_column_rows`] step the row counter and the `edi` write
cursor through a cell-column, matching the binary's `dec cl` /
`[esp+0x20]` advance. Per the §3.3 boundary, round 9 lands the
preamble's structural surface only — the per-byte unpacker dispatch
at `0x10006bac` (the high-nibble jump table) is `spec/06`'s subject,
the per-row store shapes are `spec/07` §2.2 (round 7), strip
pixel-buffer allocation is still future work per `spec/02` §10, and
the static cell-geometry-bank entry values are Extractor territory
per `spec/04` §7.1.

**Round 8 — Indeo 3 (IV31 / IV32) strip-context array + per-plane
decode-call signature.**
Round 8 adds the `indeo3::strip_context` module (`spec/02` §4–§7),
the per-codec-frame picture-decomposition state that sits between
the round-2 prelude consumer and the round-3 binary-tree walker.
[`StripGeometry::for_luma`] / `::for_chroma` resolve a plane's
strip count + per-strip widths from `(plane_width, plane_height)`
using the `ceil(W / strip_width)` and `((W-1) mod strip_width) + 1`
formulae the parser at `IR32_32.DLL!0x10003d6b` / `0x10003f53`
implements; [`strip_slot_index`] + [`StripSlotDescriptor`] surface
the §5.1 dispatchable-slot indexing (primary bank slots 3..5,
secondary bank slots 0..2, plane-role classification slots 0/3 =
luma, slots 1/2/4/5 = chroma); [`PerPlaneDecodeCall::for_plane`]
encodes the §6 seven-argument cdecl frame the picture-layer parser
hands the per-plane decoder (`IR32_32.DLL!0x10006538`) with the
codebook-bank discriminant resolved (`+0x1a00` for luma at
`IR32_32.DLL!0x100045a3`, `+0x400` for chroma at
`0x1000458d`); [`PlaneDecodeStatus`] classifies the `eax` status
code (`0` → `Ok`, `3` → `Malformed`, any other non-zero →
`Malformed`); the codec-init §7 strip-count helpers
[`luma_strip_slot_count`] / [`chroma_strip_slot_count`] (1 + 4 slot
patterns) + [`chroma_plane_height`] (luma_height / 4, `& -4`-aligned)
record the per-`ICDecompressBegin` arithmetic the future codec-init
code will consume. The per-slot field layout (`+0x00..+0x14` base
ptrs, `+0x18` strip height, `+0x1c` strip width, `+0x20..+0x3f`
strip scratch, `+0x40+` per-cell sub-array) is surfaced as the
[`slot_field`] constants module. Per the spec/02 §10 boundary,
round 8 lands the structural surface only — not the byte buffer of
the strip-context array, not the binary-tree walker's writes into
the sub-array (spec/03's subject), not the motion-compensation
reads from the pixel buffer (spec/05), and not the §5.2 sub-array
field semantics beyond `+0x1c`.

**Round 7 — Indeo 3 (IV31 / IV32) cell-shape variant inner loops.**
Round 7 lands the four cell-shape variant emission kernels
(`spec/07` §2.2 / `spec/04` §2.2) that round 6's per-position
arithmetic deferred. [`emit_variant`] runs round 6's shared
[`apply_dyad_pair`] add and then applies the per-variant store shape
the codebook DWORD's two mode bits select: variant A
([`CellVariant::Plain`], `IR32_32.DLL!0x1000670d`) stores the
dyad-pair DWORD directly to two adjacent rows (vertical doubling, no
saturation); variant B ([`CellVariant::WithEdge`], `0x10006780`)
writes one row of the per-byte [`average_7bit`] of the predictor and
the dyad result with the `0x7f7f7f7f` 7-bit clamp; variant C
([`CellVariant::DoubledRow`], `0x1000684b`) writes that average to
two rows; variant D ([`CellVariant::FullyDoubled`], `0x100068f8`)
writes the `and 0xfefefefe; shr 1` per-byte halve
([`halve_fefefefe`]) to two rows. The result is a
[`VariantEmission`] whose [`RowEmission`] `rows` lists the output
DWORD(s) to store at successive `0xb0`-stride row offsets; a
[`DyadOutcome::Fault`] emits zero rows. [`CLAMP_7BIT_MASK`]
(`0x7f7f7f7f`) and [`HALVE_CARRY_MASK`] (`0xfefefefe`) are surfaced
as constants. Per the spec/07 boundary, round 7 lands the
per-position variant store shape only — not the outer per-cell
row/column loop (the `cl` / `ch` counter walk, spec/04 §3.3), the
strip-buffer assembly, the 7→8-bit upshift, the YUV→RGB / IF09
conversion (§5), or motion compensation (`spec/05`).

**Round 6 — Indeo 3 (IV31 / IV32) output-reconstruction kernel.**
Round 6 adds the `indeo3::reconstruct` module (`spec/07` §1 + §2 +
§4), the per-position pixel-emission arithmetic that round 5's
entropy module deferred. [`apply_dyad_pair`] reproduces the
inner-loop body at `IR32_32.DLL!0x10006e0f..0x10006e2e`: the
softSIMD `predictor + primary_delta` DWORD add, the `jns` high-half
overflow test, the `xor eax, 0x80008000` back-out plus the 16-bit
`add ax, [secondary]` continuation fall-back, and the `js` fault to
error code 2 when the secondary add is still sign-set — surfaced as
[`DyadOutcome`] (`Primary` / `Continuation` / `Fault`).
[`predictor_offset`] computes the `[edi - 0xb0]` row-above predictor
address (stride [`PREDICTOR_ROW_STRIDE`] = 176), with the
top-of-strip seed pinned to the constant [`TOP_OF_STRIP_PREDICTOR`]
(`0x00`, §1.3). [`SoftSimdSum`] records both 16-bit halves'
bit-15 overflow sentinels; [`pack_predictor`] / [`unpack_pixels`]
move four pixels in and out of the little-endian softSIMD DWORD.
The 7-bit-per-byte range ([`PIXEL_VALUE_MAX`]) and the reserved
edge-marker bit ([`EDGE_MARKER_BIT`]) are surfaced as constants.
Per the spec/07 boundary, round 6 lands the per-position arithmetic
kernel only — not the per-cell-variant inner loops (A–D, §2.2), the
strip-buffer assembly, the 7→8-bit upshift, or the YUV→RGB / IF09
conversion (§5), and not motion compensation (`spec/05`).

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
| spec/02 §4.1 strip-width thresholds (160 / 40) | yes (`LUMA_STRIP_WIDTH` / `CHROMA_STRIP_WIDTH`) |
| spec/02 §4.1 remainder strip width formula | yes (`StripGeometry::last_strip_width`) |
| spec/02 §4.2 strip-count formulae (informative) | yes (`StripGeometry`) |
| spec/02 §4.4 strip height = plane height  | yes (`StripSlotDescriptor::strip_height`) |
| spec/02 §5 strip-context array layout     | yes (`STRIP_SLOT_STRIDE`, `STRIP_SLOT_COUNT`, `STRIP_ARRAY_OFFSET_IN_INSTANCE`, `STRIP_SLOT_SENTINEL`) |
| spec/02 §5.1 slot-index discipline (2 banks × 3 planes) | yes (`strip_slot_index`, `PRIMARY_BANK_SLOTS`, `SECONDARY_BANK_SLOTS`, `PlaneRole`) |
| spec/02 §5.2 per-slot field offsets (`+0x00..+0x1c`) | yes (`slot_field`) |
| spec/02 §5.2 per-slot sub-array semantics (`+0x40+`) | deferred (spec/03) |
| spec/02 §6 per-plane decode-call signature (7 args) | yes (`PerPlaneDecodeCall`) |
| spec/02 §6 codebook-bank discriminant (luma → +0x1a00, chroma → +0x400) | yes |
| spec/02 §6 plane-decode status (`eax` 0 / 3) | yes (`PlaneDecodeStatus`) |
| spec/02 §7 codec-init strip-count arithmetic | yes (`luma_strip_slot_count`, `chroma_strip_slot_count`, `chroma_plane_height`) |
| spec/02 §7 instance-state + arena sizes (`0x3010` / `0x8020`) | yes (`INSTANCE_STATE_LEN`, `PIXEL_BUFFER_ARENA_LEN`) |
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
| spec/07 §0 / §1.1 predictor address (`[edi - 0xb0]`) | yes (`predictor_offset`) |
| spec/07 §1.3 / §9 top-of-strip predictor seed (`0x00`) | yes (`TOP_OF_STRIP_PREDICTOR`) |
| spec/07 §2.1 softSIMD `predictor + delta` DWORD add | yes (`apply_dyad_pair`) |
| spec/07 §2.3 continuation / secondary-table fall-back | yes (`DyadOutcome`) |
| spec/07 §2.3 fault on still-sign-set secondary add | yes (`DyadOutcome::Fault`) |
| spec/07 §4.1 / §4.2 7-bit-per-byte range + overflow sentinel | yes (`SoftSimdSum`) |
| spec/07 §2.2 four cell-shape variant inner loops (A–D) | yes (`emit_variant`) |
| spec/07 §3 static dyad delta-table values | covered by spec/04 `DyadDeltaTable` |
| spec/07 §4.3 / §5 7→8-bit upshift + YUV→RGB / IF09 | deferred (output-buffer write) |
| spec/03 §5.4 strip-edge fix-up chroma `sar 2` (`0x10006b5e..0x10006b61`) | yes (`STRIP_EDGE_CHROMA_SHIFT`, `StripEdgeFixupDims::for_slot`) |
| spec/03 §5.4 strip-edge fix-up row stride (`0xb0`) | yes (`STRIP_EDGE_ROW_STRIDE`) |
| spec/03 §5.4 strip-edge fix-up byte-copy offsets (`[edi-1]` / `[edi]`) | yes (`STRIP_EDGE_BYTE_READ_OFFSET`, `STRIP_EDGE_BYTE_WRITE_OFFSET`) |
| spec/03 §5.4 per-row iteration over strip height       | yes (`StripEdgeRowIter`) |
| spec/03 §5.4 byte-loop pixel-buffer write              | deferred (caller pixel-buffer view) |

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
* `oxideav_indeo::indeo3::apply_dyad_pair(predictor, primary_delta,
  secondary_word) -> DyadOutcome` — the spec/07 §2.1 / §2.3 softSIMD
  `predictor + delta` add with the continuation / secondary-table
  fall-back and the §4.1 fault path. `DyadOutcome`
  (`Primary { pixels }` / `Continuation { pixels }` / `Fault`).
* `oxideav_indeo::indeo3::emit_variant(variant, predictor,
  primary_delta, secondary_word) -> VariantEmission` — the spec/07
  §2.2 / spec/04 §2.2 four cell-shape variant inner-loop store: runs
  `apply_dyad_pair`, then applies variant A (plain two-row store),
  B (`average_7bit` one-row), C (average two-row), or D
  (`halve_fefefefe` two-row). `VariantEmission { outcome, rows }`
  with `rows: RowEmission` (`::as_slice()` / `::len()` /
  `::is_empty()`) listing the output DWORD(s) at successive
  `0xb0`-stride row offsets; a `Fault` emits zero rows.
* `average_7bit(a, b) -> u32` — the §2.2 per-byte `0x7f7f7f7f`-clamped
  average (variants B / C). `halve_fefefefe(value) -> u32` — the §2.2
  `and 0xfefefefe; shr 1` per-byte halve (variant D).
* `predictor_offset(write_index) -> Option<usize>` — the §1.1
  `[edi - 0xb0]` row-above predictor address (`None` for top-row
  writes whose seed is the constant `TOP_OF_STRIP_PREDICTOR`).
* `SoftSimdSum::add(predictor, primary_delta)` (`.raw`,
  `.low_half_overflow`, `.high_half_overflow`, `.any_half_overflow()`)
  — the §2.3 / §4.1 per-half bit-15 overflow sentinel record.
* `jns_taken(u32)` — the §2.1 literal `jns` high-half test (the
  inverse of `continuation_needed`).
* `pack_predictor([u8; 4]) -> u32` / `unpack_pixels(u32) -> [u8; 4]`
  — the §0 / §2.4 little-endian softSIMD pixel-DWORD packing.
* Reconstruction constants: `PREDICTOR_ROW_STRIDE` (0xb0),
  `TOP_OF_STRIP_PREDICTOR` (0x00), `PIXEL_VALUE_MAX` (0x7f),
  `EDGE_MARKER_BIT` (0x80), `HALF_SENTINEL_MASK` (0x8000_8000),
  `CLAMP_7BIT_MASK` (0x7f7f_7f7f), `HALVE_CARRY_MASK` (0xfefe_fefe).
* VQ constants: `DYAD_TABLE_LEN`, `DYAD_BANK_COUNT`,
  `DYAD_BANK_STRIDE`, `DYAD_BANK15_VALID_ROWS`, `ARENA_LEN`,
  `ARENA_BANDS_OFFSET`, `ARENA_BAND_COUNT`, `ARENA_BAND_LEN`,
  `ARENA_HALF_LEN`, `PRIMARY_STRIDE`, `SECONDARY_STRIDE`,
  `SEED_TABLE_LEN`, `SEED_PAIR_COUNT`.
* `oxideav_indeo::indeo3::strip_slot_index(plane_idx,
  buffer_selector) -> Option<usize>` — spec/02 §5.1 dispatchable
  slot lookup; `oxideav_indeo::indeo3::StripSlotDescriptor` (`::
  for_dispatch`, `::strip_width_field_offset`,
  `::strip_height_field_offset`) typed slot view.
  `PlaneRole` (`Luma` / `Chroma` / `Scratch`, `::for_slot`,
  `::is_luma`, `::is_chroma`).
* `oxideav_indeo::indeo3::StripGeometry` (`::for_luma`,
  `::for_chroma`, `::is_aligned`, `::iter_strip_widths`) — spec/02
  §4.1 / §4.2 per-plane strip count + per-strip widths.
* `oxideav_indeo::indeo3::PerPlaneDecodeCall::for_plane(plane_idx,
  flags, bitstream_payload_offset) -> Option<PerPlaneDecodeCall>`
  — spec/02 §6 seven-argument typed cdecl view (luma / chroma
  codebook-bank discriminant + `frame_flags` bit 9 buffer
  selector). `::plane_role()`.
* `oxideav_indeo::indeo3::PlaneDecodeStatus` (`::from_eax`,
  `::is_ok`) — spec/02 §6 per-plane decoder `eax` classification.
* `oxideav_indeo::indeo3::luma_strip_slot_count(plane_width) ->
  u32` / `chroma_strip_slot_count(luma_width) -> u32` /
  `chroma_plane_height(luma_height) -> u32` — spec/02 §7
  codec-init arithmetic.
* Strip-context constants: `STRIP_SLOT_STRIDE` (0x400),
  `STRIP_SLOT_COUNT` (32), `DISPATCHABLE_SLOT_COUNT` (6),
  `STRIP_SLOT_SENTINEL` (0x1869f), `STRIP_ARRAY_OFFSET_IN_INSTANCE`
  (0x414), `INSTANCE_STATE_LEN` (0x3010), `PIXEL_BUFFER_ARENA_LEN`
  (0x8020), `INSTANCE_STRIP_ARRAY_VIEW_PTR` (0x300c),
  `INSTANCE_SECONDARY_CODEBOOK_PTR` (0x3004),
  `INSTANCE_LUMA_CODEBOOK_BANK` (0x1a00),
  `INSTANCE_CHROMA_CODEBOOK_BANK` (0x400),
  `STRIP_SLOT_BASE_PTR_COUNT` (6), `PRIMARY_BANK_SLOTS`,
  `SECONDARY_BANK_SLOTS`, `PLANE_DECODE_STATUS_OK`,
  `PLANE_DECODE_STATUS_MALFORMED`. Per-slot field offsets exposed
  as the `slot_field` constants submodule (`BASE_PTR_0..5`,
  `STRIP_HEIGHT`, `STRIP_WIDTH`, `STRIP_SCRATCH_BEGIN..END`,
  `CELL_SUBARRAY_BEGIN`).
* `oxideav_indeo::indeo3::StripEdgeFixupDims::for_slot(slot_idx,
  strip_height, strip_width) -> Option<StripEdgeFixupDims>` —
  spec/03 §5.4 strip-edge fix-up dimensions, with luma slots
  preserving the fields verbatim and chroma slots applying
  `sar 2`; `Scratch` slots yield `None`. `::row_iter()`,
  `::is_luma()`, `::is_chroma()`.
* `StripEdgeRowIter::new(height)` — non-allocating iterator
  yielding one `StripEdgeRow { row_index, row_cursor_byte_offset,
  read_offset, write_offset }` per strip row (`ExactSizeIterator`).
* `strip_edge_chroma_shift() -> u32` /
  `strip_edge_row_step() -> usize` /
  `strip_edge_byte_copy_offsets() -> (i32, i32)` — accessor
  helpers for the §5.4 constants.
* Strip-edge constants: `STRIP_EDGE_CHROMA_SHIFT` (2),
  `STRIP_EDGE_ROW_STRIDE` (0xb0), `STRIP_EDGE_BYTE_READ_OFFSET`
  (-1), `STRIP_EDGE_BYTE_WRITE_OFFSET` (0).
* Constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `FLAG_YVU9_8BIT`, `NULL_FRAME_DATA_SIZE_BITS`, `MIN_DIMENSION`,
  `MAX_WIDTH`, `MAX_HEIGHT`, `PLANE_COUNT`, `PLANE_IDX_U`,
  `PLANE_IDX_V`, `PLANE_IDX_Y`, `NUM_VECTORS_FIELD_LEN`,
  `MC_VECTOR_ENTRY_LEN`, `MIN_PRELUDE_LEN`.
* `alt_quant_indices(byte) -> (primary, secondary)` for §3.9.

## License

MIT.
