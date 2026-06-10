# oxideav-indeo

Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Round 27 — Indeo 3 (IV31 / IV32) §6.2 per-frame plane-iteration
terminator + output-reconstruction handoff (`spec/02` §6 / §6.2 /
§8).** Round 27 adds the `indeo3::frame_exit` module — the per-frame
layer above round 8's per-plane `PlaneDecodeStatus` classifier.
`PLANE_ITERATION_ORDER` (`[2, 1, 0]`) pins the §8 U → V → Y
count-down loop order with `const _` permutation cross-checks.
`PER_PLANE_DECODE_CALL_SITE_RVA` (`0x10004637`),
`PER_PLANE_DECODE_ENTRY_RVA` (`0x10006538`),
`PER_PLANE_DECODE_RET_RVA` (`0x10006b94`),
`PER_PLANE_DECODE_RET_CLEANUP_BYTES` (`0x1c`) and
`PER_PLANE_DECODE_ARG_COUNT` (`7`) pin the §6 call site, decoder
entry, and `ret 0x1c` seven-argument cdecl callee stack-cleanup
(with a `const _` cross-check that `0x1c == 7 * 4`).
`FRAME_OUTPUT_RECONSTRUCTION_RVA` (`0x10004644`) and
`FRAME_FAULT_RETURN_RVA` (`0x10006ba2`) pin the §6.2 success handoff
to the output-reconstruction stage and the §6 end-of-frame fault
path that returns the §6 status `3`. `FrameExitDisposition`
(`ProceedToReconstruction` / `EndOfFrameFault`) carries
`proceeds_to_reconstruction()` / `is_fault()` / `target_rva()` /
`frame_status()`. `FramePlaneStatusFold::from_iteration_order` /
`from_plane_idx_order` fold the three `PlaneDecodeStatus` values
into one per-frame disposition, short-circuiting on the first
faulting plane in §8 iteration order and recording both the faulting
plane's iteration index and its `plane_idx`. Per the §6 chapter
boundary the module is purely dispositional — it does not perform
the per-plane binary-tree walk (spec/03), does not classify a single
plane's `eax` (round-8 `PlaneDecodeStatus`), does not own the §6.1
per-plane payload byte budget (`PlaneByteMap`), and does not perform
the output-reconstruction stage the §6.2 handoff targets (spec/07).
14 new unit tests cover the §8 iteration order + permutation, the §6
RVA / cleanup constants, the code-memory ordering, the
reconstruction handoff, both disposition variants' getters, and the
fold across all-ok / first-plane-fault / last-plane-fault /
multiple-fault short-circuit / plane-idx reordering / order-agnostic
agreement. Total `cargo test -p oxideav-indeo` count rises to
**536 unit tests** (was 522).

**Round 26 — Indeo 3 (IV31 / IV32) §9 typed plane-data byte map
(`spec/02` §9 / §6.1 / §10 item 4).** Round 26 lands the new
`indeo3::PlaneByteMap` struct + `PictureLayer::plane_byte_map`
method — a typed view that exposes the spec/02 §9 "plane-data byte
map" diagram as absolute byte ranges into the codec-frame input
buffer. The map carries the four §9 landmarks per present plane:
the `num_vectors_range` (§3.1 / §9 row 1, the 4-byte u32), the
`mc_vectors_range` (§3.2 / §9 row 2, the `2*num_vectors`-byte
motion-vector array — empty on INTRA planes), the `payload_start`
(§3.4 / §9 row 3, the first byte of the binary-tree / VQ bitstream
payload, identical to the owning `PlanePrelude::bitstream_offset`),
and the §6.1 / §10 item 4 `payload_upper_bound` — the strict byte
budget the binary-tree decoder may scan, resolved by scanning the
OTHER present planes for the smallest `plane_base` strictly greater
than `payload_start` and falling back to `buffer_len` when no such
plane exists. The `payload_budget()` convenience exposes the §10
item 4 "end-of-plane padding tolerance" surface bridging the
structural plane layout to the (orthogonal) binary-tree walker's
actual consumption count. The map is purely structural — it does
not consult the binary-tree codes, does not own any payload bytes,
and does not alter the existing `PlanePrelude` parse — and returns
`None` for out-of-range plane indices, NULL-frame planes, and §2
skipped planes. Eight new unit tests cover the INTER-Y-plane §9
row-by-row landmarks, the INTRA empty-`mc_vectors_range` path, the
last-plane buffer_len fallback, the smallest-following-base
selection against an unsorted plane-offset triple, the non-present-
plane exclusion from the upper-bound scan, the NULL-frame +
out-of-range None paths, the `payload_start`-vs-`PlanePrelude::bitstream_offset`
cross-check, and the defensive clamp when `buffer_len <
payload_start`. Total `cargo test -p oxideav-indeo` count rises to
**522 unit tests** (was 514).

**Round 25 — Indeo 3 (IV31 / IV32) §5.4 end-of-strip edge fix-up
byte-copy executor (`spec/03` §5.4).** Round 25 closes the §5.4
executor surface the earlier rounds explicitly deferred to the
caller. `indeo3::StripEdgeFixupDims::apply_to_buffer` runs the
per-row rightmost-column duplication
(`dest[r * 0xb0 + width] = src[r * 0xb0 + width - 1]`) on a
caller-supplied `&mut [u8]` strip pixel-buffer slice, walking
`strip_height` rows at the `STRIP_EDGE_ROW_STRIDE` (`0xb0`)
per-row pointer-advance step. The §5.4 spec text describes the
fix-up at `IR32_32.DLL!0x10006b4b..0x10006b80` as a byte-level
`mov al, [edi - 1]; mov [edi], al` per row for the strip's full
height; this round translates that into a safe-Rust slice
operation. A typed `StripEdgeApplyError` enum surfaces the three
§5.4 boundary conditions: `ZeroWidthStrip` (the `[edi - 1]` source
position would precede the row's leading cursor),
`WidthExceedsRowStride` (the `[edi]` write position would land on
the next row's leading cursor, violating §5.2's strict-inside-
the-stride invariant), and `BufferTooShort` (the slice has fewer
bytes than `strip_height × 0xb0`, with both the required + the
supplied byte counts carried for diagnostics). A zero-row strip
short-circuits to `Ok(0)` without touching the buffer (matching
the §5.4 spec's `while (rows_remaining)` guard). 10 new unit tests
cover: the zero-row early return; the zero-width error; the
width-at-stride error; the buffer-too-short error; the single-row
duplication (offset 159 → 160 for a luma strip of width 160); the
chroma walk after the `sar 2` divide (a 240-row stored chroma
slot walks 60 rows at width 40, with each padding slot at offset
40 mirroring the rightmost-column byte at offset 39); the
non-padding-byte preservation invariant; the oversize-buffer
acceptance; the via-`for_slot` luma full-height walk (480 rows);
and the error-display spec-citation surface. Total `cargo test -p
oxideav-indeo` count rises to **514 unit tests** (was 504).

**Round 24 — Indeo 3 (IV31 / IV32) §7.3 reverse-decomposition
surface (`spec/05` §7.3).** Round 24 adds the
`indeo3::cell_geometry` module — the typed `(x, y, w, h)` recovery
from the round-15 [`McCellAddressPair::resolve`] outputs that
round 15 deliberately deferred ("does not perform the §7.3 `(x, y,
w, h)` recovery from the `dst_addr` byte address back into pixel
coordinates"). The surface splits the §7.3 recipe into three
sub-facets: cell shape from the `(ch, cl)` cell-state-byte
registers (`cell_w = cl_inner * 4` / `cell_h = row_band_count *
4`), cell top-left coordinates from `dst_addr` (`cell_x = dst_addr
mod 0xb0`, `cell_y = (dst_addr - strip_base) / 0xb0`), and the
typed [`CellRect`] composition. [`CELL_PIXELS_PER_COLUMN_GROUP`]
(`4`) and [`CELL_PIXELS_PER_ROW_BAND`] (`4`) alias the §5.1
[`MC_COLUMN_GROUP_PIXELS`] / [`MC_BAND_ROWS`] inner-loop-kernel
constants via `const _` cross-checks so the §7.3 reverse surface
does not have to reach into the kernel constants directly.
[`cell_width_from_column_group_count`] /
[`cell_height_from_row_band_count`] apply the §2.4 minimum-cell-size
zero-input rejection and `u32` overflow guards.
[`row_band_count_from_ch_register`] surfaces the §7.3 / §7.1
`ecx >> 24` upper-byte extraction from the initial `ch` register
snapshot. [`CellCoords`] / [`cell_coords_from_dst_addr`] perform
the §7.3 modular decomposition against [`MC_ROW_STRIDE`], with
defensive `None` on caller-contract violation (`dst_addr <
strip_base`). [`CellRect::from_parts`] / [`reverse_decompose`]
compose the three sub-facets into the full §7.3 shape descriptor,
with a typed [`CellRectDecodeError`] surface for the four failure
modes (dst-address-below-strip-base, zero column-group count,
zero row-band count, dimension overflow). Per the §7.3 chapter
boundary, the module accepts pre-resolved `cl_inner` bytes (the
codebook-bank `bank[+0x000][cl]` LUT values are §7.5 Extractor
territory), leaves strip-pixel-buffer-to-frame composition to
`spec/07 §5.7`, and leaves visible-width classification to
[`McPlaneRole::strip_visible_width`]. 34 new unit tests cover: the
two factor constants pinned to the §5.1 surfaces;
`cell_width_from_column_group_count` at one column-group, full-
strip width (44 → 176 px), typical chroma-cell width (10 → 40
px), zero-input rejection, and max-byte arithmetic;
`cell_height_from_row_band_count` at typical heights (1 → 4, 10
→ 40) with zero-input rejection and max-byte arithmetic;
`row_band_count_from_ch_register` extraction across four
bit-patterns including lower-three-byte masking and zero-`ch`
edge case; `cell_coords_from_dst_addr` at strip origin, within
first row, one row below base, last column of strip row (175,
0), arbitrary position (16, 7) via 7 * 0xb0 + 16 offset, and the
caller-contract violation (`dst_addr < strip_base`);
`CellRect::from_parts` at full-strip 176×40 and typical 16×16
INTRA cells with per-factor error propagation; `reverse_decompose`
end-to-end composition at strip origin + arbitrary (8, 4) 8×8
cell with the four-way error fan-out; cross-module identities
pinning the `MC_ROW_STRIDE` modulus alignment, the
`MC_COLUMN_GROUP_PIXELS` / `MC_BAND_ROWS` factor equivalence, and
a forward-reverse round-trip at arbitrary `(cell_x, cell_y) =
(24, 9)` coordinates. Total `cargo test -p oxideav-indeo` count
rises to **504 unit tests** (was 470).

**Round 23 — Indeo 3 (IV31 / IV32) §6 picture-layer plan → 7-
argument per-plane decode-call bridge (`spec/02` §6).** Round 23
adds the typed accessor `indeo3::PlaneDecodePlan::to_decode_call()`
that maps a parsed picture-layer plan to a populated
`indeo3::PerPlaneDecodeCall` (the §6 7-argument cdecl frame the
per-plane decoder consumes at `IR32_32.DLL!0x10006538`) without
re-traversing the picture layer. The bridge delegates to a new
sibling constructor `PerPlaneDecodeCall::for_plane_and_buffer
(plane_idx, buffer_selector, bitstream_payload_offset)` that takes
the spec/02 §3.2 / §5.1 buffer-selector bit directly instead of
round-tripping through `FrameFlags`; the existing
`PerPlaneDecodeCall::for_plane(plane_idx, flags, payload)` keeps
its `FrameFlags` signature and now delegates to the bool-direct
constructor. Both constructors apply the §6 codebook-bank
discriminant (luma → `+0x1a00`, chroma → `+0x400`), populate the
§6 constants for the strip-context array view (`+0x300c`) and the
secondary codebook pointer (`+0x3004`), and per spec/02 §10 item 3
set `slot_idx_src == slot_idx_dst`. 6 new unit tests cover: a
PRIMARY-bank luma plan bridging to a slot-3 / `+0x1a00` /
luma-role call frame with bitstream_payload_offset equal to the
plan's `bitstream_offset` (§6); chroma V/U plans on a 320×240
picture surfacing the `+0x400` chroma bank at primary slots 4 / 5
respectively (§5.1 + §6); a SECONDARY-bank Y plan surfacing slot 0
with the luma bank still `+0x1a00` (§5.1 + §6 luma-vs-chroma
discriminant keys on plane_idx not the buffer bit); cross-check
equality of the bridge constructor against the `FrameFlags`
constructor over all three planes; cross-check equality of
`for_plane_and_buffer` against `for_plane` for the four `frame_flags
& 0x0205 / 0x0200 / 0x0210` permutations × three plane indices ×
four payload offsets (12+ assertion pairs); and out-of-range
rejection at `plane_idx == PLANE_COUNT` for the new bool-direct
constructor under both `buffer_selector` polarities. Total
`cargo test -p oxideav-indeo` count rises to **470 unit tests**
(was 464).

**Round 22 — Indeo 3 (IV31 / IV32) §4 picture-layer → §5 strip-
context decode-plan bridge (`spec/02` §4 + §5 + §6).** Round 22
adds the typed accessor `indeo3::PictureLayer::plane_decode_plan`
that bundles, for one parsed plane, the §4 [`StripGeometry`] (plane
width / height, per-plane-class strip width, strip count, §4.1
remainder-formula last-strip width), the §5.1 / §5.2
[`StripSlotDescriptor`] (slot index keyed by `(plane_idx,
buffer_selector)`, plane role, per-slot `STRIP_WIDTH` /
`STRIP_HEIGHT` field offsets), and the §3.4 bitstream-payload
offset + §3.1 `num_vectors` from the round-2 prelude parser at one
typed entry point ([`indeo3::PlaneDecodePlan`]). The accessor:
applies the §4 picture-decomposition table for the chroma planes
(`(luma_width / 4, chroma_plane_height(luma_height))`), surfaced via
the new `indeo3::chroma_plane_width` helper that explicitly does
**not** apply the §7 item 4 `& -0x4` height-alignment mask the
height helper applies; for a single-strip plane (§4.2 row 1,
`W ≤ strip_width`) writes the §4.1 remainder width
(`((W-1) mod strip_width) + 1`, = the picture width itself) into
the slot descriptor's `STRIP_WIDTH` field per §5.2; returns `None`
for any plane_idx ≥ 3, for any [`PlanePresence::NullFrame`] /
`Skipped*` plane (no decode call to plan), and bundles
[`PlaneRole`] / `is_luma()` / `is_chroma()` / `is_intra()`
predicates on the plan itself. 8 new unit tests cover: §4 luma
geometry on a 320×240 picture (slot 3, strip_count 2, aligned), §4
chroma subsampled geometry on the V plane of a 320×240 picture
(slot 4, plane width 80, plane height 60, INTER with 2 motion
vectors), §4.2 row 1 single-strip remainder-width path on a
144×112 picture (slot's `STRIP_WIDTH` = 144, not the 160 constant),
§5.1 secondary-bank slot remapping (Y → 0, V → 1, U → 2 when
`frame_flags` bit 9 is set), `None` for every NULL-frame plane,
`None` for a skipped plane while sibling planes still return a
plan, `None` for out-of-range plane indices, and the
`chroma_plane_width` divide-by-4-without-alignment behaviour on
luma widths 0, 4, 16, 17, 18, 22, 160, 320, 640. Total
`cargo test -p oxideav-indeo` count rises to **464 unit tests**
(was 456).

**Round 21 — Indeo 3 (IV31 / IV32) §5.6 MC fetcher → VQ residual
chapter boundary surface (`spec/05` §5.6).** Round 21 adds the
`indeo3::mc_residual_boundary` module, the typed §5.6 disposition
surface that pins the MC chapter's terminator and the spec/06
entropy chapter's start point. `MC_FETCHER_LAST_WRITE_RVA`
(`= 0x1000_6732`) pins the §5.6 second paragraph "this chapter's
last instruction for the cell is the final `[edi + 0x20c] = eax`
of the MC fetcher's inner loop", with a `const _` cross-check that
the RVA lies within the §5.1 inner-loop range
`0x1000670d..0x1000673d`. `MC_FETCHER_LAST_WRITE_DST_OFFSET`
(`= 0x20c`) pins the row-3 destination byte offset, with a `const _`
cross-check that the offset equals `MC_FULL_PEL_ROW_OFFSETS[3]`
(`= 0x210`) minus the §5.1 `lea edi, [edi + 0x4]` mid-loop column
advance. `MC_CHAPTER_LAST_DST_ROW_INDEX` (`= 3`) pins the §5.1
band's fourth-and-last row index, with a `const _` cross-check
that `MC_CHAPTER_LAST_DST_ROW_INDEX + 1 == MC_BAND_ROWS`.
`VQ_RESIDUAL_DISPATCH_RVA` (`= 0x1000_6bac`) pins the §5.6 first
paragraph + `spec/04 §3.4` per-byte unpacker dispatch entry where
spec/06 picks up the residual-application chain, with a `const _`
cross-check that the RVA strictly follows `MC_FETCHER_LAST_WRITE_RVA`
in code memory. `shares_destination_buffer()` (`const`-`true`)
surfaces the §5.6 first-paragraph disposition that the MC
prediction and the VQ residual share the same destination buffer
(the residual is added in place; no per-cell intermediate copy).
The `McCellDisposition` enum (`PredictionOnly` /
`PredictionThenResidual`) classifies the §5.6 first-paragraph
two-path post-MC chain: a pure-prediction INTER cell whose
chained-flag arithmetic does not re-enter VQ_TREE vs an INTER cell
whose `spec/04 §7.5` chained-flag re-entry triggers an in-place
residual; `requires_residual()` and `residual_application()` are
the typed predicates. The `ResidualApplication` enum (`None` /
`InPlaceOverPrediction`) carries the residual-application
classification with `is_none()` / `is_in_place()` predicates. The
`McToVqHandoff` composite struct bundles the MC-chapter terminator
RVA with the spec/06 start RVA at a single typed surface;
`McToVqHandoff::for_disposition(disp)` returns the populated
handoff for `PredictionThenResidual` and `None` for
`PredictionOnly` (the latter case ends the cell at the MC chapter
terminator without spec/06 dispatch); `rva_delta()` returns the
positive byte distance between the two RVAs. 25 new unit tests
cover the four RVA / offset constants (3 + 3 + 2 + 3), the
shared-destination-buffer disposition (1), the `McCellDisposition`
predicates and variants (4), the `ResidualApplication` predicates
and variants (3), the `McToVqHandoff::for_disposition` happy paths
and round-trip identity (5), and cross-module sanity (3: the row-
offset table re-use, mode-agnostic terminator across the §2.2
four-way fork, and the two-path partition of the post-MC chain).
Per the §5.6 chapter boundary, the module deliberately does not
perform the MC fetcher's inner-loop reads / writes (owned by
`mc_kernel`), does not perform the per-byte mode read at
`IR32_32.DLL!0x10006bac` (owned by the spec/06 unpacker dispatch
in `entropy`), does not perform the VQ residual addition itself
(spec/06 unpacker territory), does not classify a cell-state byte
as chained or unchained (`spec/04 §7.5` territory; this module
accepts a pre-classified `McCellDisposition` from the caller), and
does not own the §5.1 inner-loop row layout (owned by
`MC_FULL_PEL_ROW_OFFSETS`; this module re-uses the final entry
through `MC_FETCHER_LAST_WRITE_DST_OFFSET`). Total `cargo test -p
oxideav-indeo` count rises to **456 unit tests** (was 431).

**Round 20 — Indeo 3 (IV31 / IV32) §5.5 chroma-plane scaling
surface (`spec/05` §5.5).** Round 20 adds the `indeo3::mc_chroma`
module, the typed §5.5 disposition surface that pins the MC
fetcher's behaviour on the chroma slot indices `1, 2, 4, 5`
relative to the luma slot indices `0, 3`. The §5.5 four-bullet
disposition is: (1) the MC inner-loop kernel geometry is
plane-role-invariant, (2) the strip's allocated row stride
remains the constant `0xb0` for chroma (not the visible 40), (3)
the codebook-bank cell-size populations are 4:1 / 4:1 subsampled
in each axis when the slot is chroma, and (4) the packed-MV's
`176`-factor is a buffer-allocation constant, not a
plane-resolution constant — applies uniformly across luma and
chroma. `LUMA_PIXEL_PER_CHROMA_PIXEL` (`= 4`) pins the §5.5
third-bullet YVU9 subsampling ratio with a `const _` cross-check
against the macroblock-layer `LUMA_STRIP_WIDTH` /
`CHROMA_STRIP_WIDTH` split (`160 == 40 * 4`).
`CHROMA_PACKED_MV_FACTOR_IS_BUFFER_STRIDE` (`= true`) surfaces
the §5.5 fourth-bullet disposition with a `const _` cross-check
that `MV_PIXEL_OFFSET_ROW_STRIDE == MC_ARENA_ROW_STRIDE`.
`MC_KERNEL_GEOMETRY_IS_PLANE_ROLE_INVARIANT` (`= true`,
re-exported as `McKernelGeometryIsPlaneRoleInvariant`) pins the
§5.5 first-bullet disposition that the MC fetcher's inner-loop
geometry constants are not parameterised on plane role. The
`MvPixelOffsetInterpretation` enum carries the §5.5 fourth-bullet
disposition's single variant
`LumaOrChromaUniformBufferStride` with `pixel_offset_row_stride()`
returning the §3.3 row-stride factor `0xb0`. The local
`McPlaneRole` enum (`Luma` / `Chroma`) carries the §5.1 slot-
index split for the MC fetcher only (separate from
`strip_context::PlaneRole` which carries different invariants);
`from_strip_slot_index(slot)` classifies `0, 3` ⇒ `Luma`,
`1, 2, 4, 5` ⇒ `Chroma`, other ⇒ `None`. `strip_visible_width()`
returns `160` (luma) or `40` (chroma); `strip_allocated_row_stride()`
returns `0xb0` for both (the §5.5 second-bullet disposition);
`cell_size_subsampling_ratio()` returns `1` (luma) or `4`
(chroma); `is_luma()` / `is_chroma()` are disjoint predicates;
`chroma_cell_size(luma_width, luma_height) -> Option<(u32, u32)>`
applies the §5.5 third-bullet integer-multiple 4:1 / 4:1
subsampling (returns `None` for non-multiple inputs). 30 new unit
tests cover the subsampling-ratio constant (2), the packed-MV
buffer-stride disposition (2), the kernel-geometry invariance
flag and its constants (2), the `MvPixelOffsetInterpretation`
enum (2), the slot-index classifier across luma / chroma /
out-of-range / full in-range coverage (4), the visible-width and
row-stride getters (5), the subsampling-ratio getters and role
predicates (3), the `chroma_cell_size` happy paths / rejections
/ zero-edge (6), and cross-module sanity (4). Per the §5.5
chapter boundary, the module deliberately does not perform the
codec-init population of the codebook-bank `+0x000` / `+0x100`
sub-tables (host-side per `spec/04 §5.3`), does not perform the
§5.1 inner-loop reads / writes (owned by `mc_kernel`), does not
perform the §2.3 source-pointer arithmetic (owned by
`apply_mv_source_offset`), and does not derive the luma vs
chroma slot-index split itself beyond the §5.1 cross-reference.

**Round 19 — Indeo 3 (IV31 / IV32) §4.4 "no explicit boundary
check" surface (`spec/05` §4.4).** Round 19 adds the
`indeo3::mc_bounds` module, the typed §4.4 disposition surface
that pins the *absence* of a boundary check on the §2.3 source-
pointer arithmetic — the §4.4 paragraph 1 disposition "the parser
does not validate that `pixel_offset` (the high 30 bits of the
packed MV, signed) addresses a byte within the source strip's
allocated buffer". `MC_NO_BOUNDARY_CHECK` (`= true`) surfaces the
disposition as a typed `const`-`true` flag callers reference at
the call site so the disposition is greppable from any audit of
the §2.3 [`apply_mv_source_offset`] call graph. The
`SourcePointerBoundsCheck` enum (`BinaryDoesNotCheck` /
`CallerOptsIn`) names the two documentation-time call-site
paths: the binary path (no check) vs the safe-Rust opt-in path
(invoke the §4.4 paragraph 3 classifier before the §2.3 add). The
`MvSourceOffsetClass` enum (`InRegion` / `OutOfRegion` /
`Underflow`) classifies a §4.4 source-pointer offset against a
supplied strip region: `InRegion` ⇒ the §5.1 MC fetcher reads
valid strip-pixel bytes; `OutOfRegion` ⇒ the §4.4 paragraph 1
"decoder reads from whatever bytes happen to occupy that part of
the heap arena" case; `Underflow` ⇒ the signed `add esi, edx`
goes below zero. `mv_source_offset_in_strip_region(dst_cell_base,
mv_offset, strip_region_bytes_total)` runs the §4.4 paragraph 3
classification in one `const` entry point, separately from the
§2.3 arithmetic itself. `STRIP_REGION_LUMA_240_BYTES` (`= 0xa500`
= 42_240) pins the §4.4 paragraph 2 first-bullet worked-example
region size (`0xb0 * 240` for a 240-pixel-tall luma plane) with
`const _` cross-checks against the §4.1 `strip_region_bytes(240)`
formula and the §4.4 prose's explicit `0xa500` / decimal `42_240`
figures. `STRIP_REGION_LUMA_240_FITS_IN_ARENA` (`= false`) pins
the §4.1 footnote discrepancy that the §4.4 prose's "far smaller
than the 0x8020-byte arena's total" claim does *not* hold
numerically (`0xa500 > 0x8020`), matching round 17 mc_arena's
`StripArenaCapacity::fits_in_arena` disposition. The
`PaddingPixelPreservation` enum (`DeterministicAtCodecInit` /
`PreservedAcrossFramesByStripEdgeFixup`) carries the §4.4
paragraph 2 second-bullet two-half disposition as a typed
surface linking `spec/02 §7` codec init to round 11's
`StripEdgeFixupDims`. 27 new unit tests cover the disposition
flag (1, with `core::hint::black_box` defeating clippy's
constant-folding lint), the worked-example constants and the
arena-discrepancy assertion (3), `SourcePointerBoundsCheck`
predicates (3), `MvSourceOffsetClass` predicates (3),
`mv_source_offset_in_strip_region` happy paths (3),
out-of-region edges (4), underflow edges (3), zero-size region
(1), saturating add at `u64::MAX` (1), `PaddingPixelPreservation`
predicates (3), and cross-module sanity (2). Per the §4.4
chapter boundary, the module deliberately does not perform the
§2.3 source-pointer arithmetic itself (owned by
`apply_mv_source_offset`), does not own the strip allocator or
its deterministic-pattern fill (host-side per `spec/02 §7`),
does not perform the §5.4 strip-edge fix-up (owned by
`StripEdgeFixupDims` / `StripEdgeRowIter`), does not range-check
`dst_cell_base` itself against the strip region (assumed in-range
from the §7.2 `mc_dest_address` chain), and never indicates a
malformed stream — per §4.4 the binary "tolerates [out-of-region
MVs] without faulting; they are not malformed from the decoder's
perspective".

**Round 18 — Indeo 3 (IV31 / IV32) §4.3 source-pointer plumbing
(`spec/05` §4.3).** Round 18 adds the `indeo3::mc_source_plumbing`
module, the typed surface for the per-plane decoder →
cell-state dispatcher stack-frame hand-off the §4.3 four-instruction
fragment at `IR32_32.DLL!0x10006638..0x10006641` runs:
`sub eax, edi; add eax, [esp + 0x54]; mov edx, [esi + 4 * eax];
mov [esp + 0x24], edx`. Round 16 (`bank_select`) resolved the §4.2
`(dst_slot, src_slot)` pair; round 15 (`mc_address`) resolved the
§7.2 cell-data DWORD load; round 17 (`mc_arena`) pinned the §4.1
arena the six per-slot base pointers point into. Round 18 owns the
§4.3 stack-frame link between those three.
`DECODER_ARG_SRC_SLOT_OFFSET` (`= 0x54`) /
`DECODER_ARG_DST_SLOT_OFFSET` (`= 0x58`) pin the two per-plane
decoder argument byte-offsets the §4.2 inversion at
`IR32_32.DLL!0x100045c3..0x100045fd` writes into;
`DISPATCHER_SCRATCH_SRC_DATA_OFFSET` (`= 0x24`) /
`DISPATCHER_SCRATCH_DST_DATA_OFFSET` (`= 0x28`) /
`DISPATCHER_SCRATCH_EXTRA_OFFSET_OFFSET` (`= 0x38`) pin the three
cell-state dispatcher scratch slots the §4.3 / §7.2 fragments write
the resolved cell-data DWORDs to;
`STRIP_CTX_ARRAY_ELEMENT_SHIFT` (`= 2`) surfaces the `mov edx, [esi
+ 4 * eax]` element-to-byte shift. `const _` cross-checks pin the
`+ 4` adjacency between the source / destination decoder-arg slots
and between the source / destination cell-data dispatcher scratch
slots. The `DecoderStackArg` enum (`SrcSlot` / `DstSlot`) typed-
picks one of the two decoder arguments with `byte_offset()`,
`role()` (returning the [`CellAddrRole`] surface from round 15),
and `dispatcher_scratch()` linking it to its companion
[`DispatcherScratch`] cell-data slot; the `DispatcherScratch` enum
(`SrcCellData` / `DstCellData` / `ExtraOffset`) typed-picks one of
the three scratch slots with `byte_offset()`, `role()`, and
`is_source_companion()` (`true` only for `ExtraOffset`, the §7.2
`idx_src + 1` companion that the §5.5 boundary fix-up consumes).
`SourcePlumbingPair::for_role` runs the §4.3 mapping in one entry
point and returns the typed `(decoder_arg, dispatcher_scratch)`
pair whose two halves share the same `CellAddrRole`.
`is_self_copy_degenerate(dst_slot, src_slot)` surfaces the §4.3
closing predicate "`dst_slot == src_slot` ⇒ self-copy" — the §4.3
prose notes "no such frame is observed in the binary", and the
predicate cross-validates against `McBankAssignment::is_self_copy`
on every well-formed §4.2 inversion (always `false`). 33 new unit
tests cover the five offset constants (3 + 3 + 3 distinct-slots /
adjacency / spec-match), the element-index shift identity (1), the
two `DecoderStackArg` variants' `byte_offset` / `role` / paired-
`dispatcher_scratch` getters (6), the three `DispatcherScratch`
variants' `byte_offset` / `role` / `is_source_companion`
predicates (7), the `SourcePlumbingPair::for_role` round-trip
identity over both roles (5), the `is_self_copy_degenerate`
predicate over equal slots (1) / distinct slots (1) / every
`McBankAssignment::resolve` output (1) / the
`McBankAssignment::is_self_copy` agreement (1), and the
scratch-vs-arg cross-frame disjoint-ranges documentation
invariant (1). Per the §4 chapter boundary, the module
deliberately does not perform the cell-data DWORD load itself
(owned by `mc_address`), does not resolve `(dst_slot, src_slot)`
(owned by `bank_select`), does not perform the §2.3 source-pointer
arithmetic (owned by `apply_mv_source_offset`), and does not
enforce per-strip bounds (per §4.4 the binary itself does not
either).

**Round 17 — Indeo 3 (IV31 / IV32) strip pixel-buffer arena
geometry (`spec/05` §4.1).** Round 17 adds the `indeo3::mc_arena`
module, the typed §4.1 surface that links round 8's strip-context
slot layout (the six base-pointer fields at `[ctx+0x00..+0x14]`)
to round 15's `mc_address` cell-position decoding entry (the
per-cell `dst_cell_data` / `src_cell_data` DWORDs the MC fetcher
consumes). `MC_ARENA_LEN` (`= 0x8020`) aliases the round-8
`PIXEL_BUFFER_ARENA_LEN` heap-block size from
`IR32_32.DLL!0x10003cdc..0x10003ce3`, with a `const _` cross-check;
`MC_ARENA_ROW_STRIDE` (`= 0xb0`) is `const _`-checked against
both `mc_kernel::MC_ROW_STRIDE` and
`reconstruct::PREDICTOR_ROW_STRIDE`. `STRIP_PIXEL_BUFFER_ALIAS_COUNT`
(`= 6`) re-exports the §4.1 "six aliases of the strip's pixel
buffer" identity by its §4.1 name; the
`StripPixelBufferAlias` enum (`Base0` / `Base1` / `Base2` /
`Base3` / `Base4` / `Base5`) gives the typed pick of one of the
six aliases with `from_index(0..6) -> Option<Self>`, `as_index()`,
and `slot_relative_byte_offset()` returning one of
`slot_field::BASE_PTR_{0..5}`. `strip_region_bytes(plane_height)`
runs the §4.1 worked-example arithmetic
`MC_ARENA_ROW_STRIDE * plane_height_pixels` in `u64`;
`StripArenaCapacity::for_plane_height` pins the §4.1 footnote
predicate `region_bytes <= MC_ARENA_LEN` (yielding the boundary
height `MC_ARENA_LEN / MC_ARENA_ROW_STRIDE = 186`, with the §4.1
worked-example height 240 flagged as not fitting — surfacing the
arithmetic discrepancy the §4.1 prose mentions between the arena
size and the per-strip region size). `base_pointer_aliases_equal`
encodes the §4.1 / `spec/03 §5.2` "six pointers are aliases of
the same per-strip region" invariant as a `slot_bytes: &[u8] ->
Option<bool>` over the six little-endian DWORDs at the slot-
relative offsets, returning `None` if the slice does not extend
through the last base-pointer field. 21 new unit tests cover the
§4.1 arena-geometry constants (3), the alias enum's round-trip
indexing and out-of-range rejection (4), the alias byte offsets
against `slot_field::BASE_PTR_*` and the 4-byte-apart DWORD-
alignment invariant (3), the boundary-with-slot-stride identity
(1), the `strip_region_bytes` worked example / zero-height /
no-wrap-on-u32-MAX cases (3), the `StripArenaCapacity` boundary-
height arithmetic and the §4.1 worked-example "does not fit"
case (4), the `base_pointer_aliases_equal` well-formed /
malformed / short-slice / boundary-slice cases (4), and inter-
module row-stride cross-checks linking `mc_arena` to
`mc_packed::MV_PIXEL_OFFSET_ROW_STRIDE` and
`cell_subarray::PER_CELL_EDGE_ROW_STRIDE` (2). Per the §4 chapter
boundary, the module deliberately does not perform the heap
allocation itself (the `IR32_32.DLL!0x10003cdc` call is host
`LocalAlloc` territory), does not enforce per-strip bounds at MC-
fetcher time (§4.4 the binary itself does not range-check the
§2.3 source-pointer arithmetic), does not own or populate the
slot's six base-pointer fields (codec-init at
`IR32_32.DLL!0x10003edc..0x10003f3a` writes them), does not
perform the §4.2 ping-pong bank pick or the §4.3 source /
destination slot inversion (owned by `bank_select`), and does
not own the arena's per-frame contents (those are written by
`mc_kernel` and `reconstruct`).

**Round 16 — Indeo 3 (IV31 / IV32) ping-pong bank selection
(`spec/05` §4.2).** Round 16 adds the `indeo3::bank_select` module,
the typed surface for the `frame_flags` bit 9 source / destination
slot inversion the per-plane decoder builds at
`IR32_32.DLL!0x100045b1..0x100045fd` before pushing the
`[esp+0x54]` / `[esp+0x58]` arguments to the binary-tree walker.
`BANK_INVERSION_DELTA` (`= 3`) surfaces the §4.2 "plane_idx + 3"
identity as a named constant aliased to
`PRIMARY_BANK_SLOTS[i] - SECONDARY_BANK_SLOTS[i]` (cross-checked
per plane). The `Bank` enum (`Primary` / `Secondary`) carries
`Bank::from_buffer_selector` (decodes `frame_flags` bit 9 via
`FrameFlags::buffer_selector()`, matching the parser's
`test ch, 0x2` on the `frame_flags` high byte), `Bank::opposite()`
(involution, Primary ⇔ Secondary), and
`Bank::slot_for_plane(plane_idx)` (with the
`plane_idx >= PLANE_COUNT` guard matching `strip_slot_index`).
`McBankAssignment::resolve(flags, plane_idx)` runs the §4.2
mapping in one entry point and returns the resolved
`(dst_slot, src_slot, dst_bank)` triple with the source bank
wired to `dst_bank.opposite()`; `is_self_copy()` flags the §4.2
"never observed in the binary" same-bank degenerate case (always
`false` for a well-formed result), and `slot_delta()` is
identically `BANK_INVERSION_DELTA` for any `resolve()` output.
Per §4.2 the destination is the bank the *current* frame writes
into and the source is the bank the *previous* frame wrote into
(the MC "previous frame" reference); the ping-pong invariant
holds between consecutive frames whose bit 9 flips — frame N's
`dst_slot` is frame N+1's `src_slot`. 28 new unit tests cover
`BANK_INVERSION_DELTA` cross-checks per plane (4), the `Bank`
constructor against the §4.2 bit-9 / parser convention including
the "other bits irrelevant" rule (3), `Bank::opposite` involution
(2), the `is_primary` / `is_secondary` partition (1),
`Bank::slot_for_plane` against the spec/02 §5.1 tables across all
three planes (3), the resolved triple for each of the six legal
`(bit-9, plane)` combinations (6), the `is_self_copy()` /
`slot_delta()` invariants (3), agreement with the round-8
`strip_slot_index` for both destination and inverted source (2),
the source-bank-is-dst-bank-opposite identity (1), out-of-range
`plane_idx` rejection at the resolver (1), and the ping-pong
two-frame identity for slots and banks (2). Per §4.2 the module
deliberately does not perform the strip-context-slot read (that's
`mc_address::CellSubarrayIndex`), does not load the per-cell
sub-array DWORDs, and does not own the per-frame bank-state
machine that flips bit 9 across frames (the encoder owns that
sequence; the decoder just consults the per-frame value).

**Round 15 — Indeo 3 (IV31 / IV32) cell-position decoding entry
(`spec/05` §5.4 / §7.2).** Round 15 adds the `indeo3::mc_address`
module, the bridge between round 14's MC fetcher inner-loop
kernel (which consumes two pixel-buffer byte addresses
`dst_addr` / `src_addr`) and the cell-state dispatcher's index-
arithmetic chain that produces them. The §7.2 / §4.3
`shl eax, 0x4` step at `IR32_32.DLL!0x10006615` is surfaced as
[`CELL_SLOT_STRIDE`] (`16`) with the §7.2 "cell-slot index 0..15"
upper bound as [`CELL_SLOT_INDEX_MAX`] (`15`).
[`CellSlotBase::from_bank_byte`] applies the post-`shl 0x4` step
to the raw `bank[+0x200][ch]` one-byte lookup;
[`CellSubarrayIndex::dst`] / [`CellSubarrayIndex::src`] compose
`idx_dst = 16 * cell_slot + dst_slot` /
`idx_src = 16 * cell_slot + src_slot` (the per-cell sub-array
element indices loaded at `0x10006638..0x10006641`).
[`CellAddrEntry::dst`] / [`CellAddrEntry::src`] hold the
destination / source cell-data DWORDs tagged with their
[`CellAddrRole`] (`Dest` / `Src`) and carry the §7.2 `[esp+0x38]`
extra-offset companion on the source-role branch.
[`mc_dest_address`] composes the §5.4 / §7.2
`dst_addr = dst_cell_data + bank[+0x700][cl]`;
[`mc_source_address`] composes
`src_addr = src_cell_data + bank[+0x700][cl] + sign_extend(packed_MV >> 2)`
by chaining the §5.4 cell-base add with the §2.3
[`apply_mv_source_offset`] sign-extending MV displacement.
[`McCellAddressPair::resolve`] runs the complete §7.2 chain in
one entry point; [`McAddressError`] enumerates the four safe-
Rust check failures (destination overflow, source overflow, MV
under/overflow, role mismatch). The `is_self_copy()` predicate
flags the §8.2 item 8 identity-MV degenerate case
(`dst_slot == src_slot` + `packed_mv == 0` →
`dst_addr == src_addr`). 29 new unit tests cover the §7.2 / §4.3
cell-slot stride constants (3), the [`CellSlotBase`] in-range vs
out-of-range predicate at the byte boundary (4), the
[`CellSubarrayIndex`] composition with the §4.2 ping-pong
`dst_slot - src_slot` delta and the byte-offset = element × 4
cross-check (4), the [`CellAddrEntry`] role-tagged shape (2),
the [`mc_dest_address`] / [`mc_source_address`] composition with
identity / positive / negative displacements and `usize` wrap /
signed underflow rejections (7), the complete
[`McCellAddressPair::resolve`] chain including swapped-role
rejection and all four [`McAddressError`] propagation modes plus
the §8.2 item 8 self-copy case (8), and a `CELL_STACK_ENTRY_SIZE`
consistency check linking [`CellSubarrayIndex::byte_offset`] to
the existing 4-byte-per-entry constant (1). Per the §5.4 / §7
chapter boundary, the module deliberately does not own the
`bank[+0x200]` or `bank[+0x700]` table values (§7.5 Extractor
territory), does not own the strip-context per-cell sub-array
DWORDs (spec/03 §6 open question 4 pre-frame setup), does not
perform the §7.2 `[esp+0x34]` boundary-fix-up reduction, does
not perform the §7.3 reverse `(x, y, w, h)` decomposition, and
does not perform the §4.2 `frame_flags` bit 9 source /
destination slot inversion.

**Round 14 — Indeo 3 (IV31 / IV32) motion-compensation cell-copy
inner-loop kernel (`spec/05` §5.1 / §5.2 / §5.3).** Round 14 adds
the `indeo3::mc_kernel` module, the next slice in the MC pipeline
after round 13's packed-MV decode: once the source-pixel base
address has been resolved (round 13's
`PackedMv::source_address` /
`apply_mv_source_offset`), the per-cell copy kernel reads four
DWORDs (= 16 bytes) per inner-loop iteration from successive rows
of the source buffer and writes them into the corresponding rows
of the destination cell. The §5.1 full-pel inner-loop shape is
captured by `MC_ROW_STRIDE` (`0xb0`),
`MC_INNER_LOOP_DWORDS_PER_ITER` (`4`),
`MC_INNER_LOOP_BYTES_PER_ITER` (`16`), `MC_BAND_ROWS` (`4`),
`MC_BAND_BYTE_STRIDE` (`0x2c0`) and `MC_COLUMN_GROUP_PIXELS` (`4`);
the four hard-coded source-byte offsets at
`IR32_32.DLL!0x1000670d..0x1000673d` are surfaced as
`MC_FULL_PEL_ROW_OFFSETS = [0, 0xb0, 0x160, 0x210]` and through
the per-row helper `mc_full_pel_row_dword` / typed
`McKernelStep::for_row`. `McKernelGeometry::new(width_px,
height_px)` enforces the §5.1 multiple-of-4 width/height
invariants and the §5.3 row-stride bound
(`MC_MAX_CELL_WIDTH_BYTES` = `0xb0`). The §5.2 per-DWORD
averaging kernels — `mc_vert_half_pel_pair` for the `01` path
(`(src[i] + src[i + 0xb0]) >> 1` via the shared `average_7bit`
SWAR identity, `MC_VERT_HALF_PEL_NEIGHBOUR_OFFSET = 0xb0`),
`mc_horiz_half_pel_pair` for the `10` path (`(src[i] + src[i +
1]) >> 1` with the in-DWORD byte splice
`(src_dword >> 8) | (src_dword_next << 24)`,
`MC_HORIZ_HALF_PEL_NEIGHBOUR_OFFSET = 1`), and
`mc_both_half_pel_quad` for the `11` path (the §2.2 / §5.2 2×2
unweighted box filter, composed horizontal-pair-first /
vertical-pair-second) — share the same `(a + b) >> 1`
byte-parallel identity with `reconstruct::average_7bit`,
confirming the §2.2 "no separate filter coefficient tables"
disposition. 31 new unit tests cover the §5.1 / §5.3 constants
(8), the geometry-construction invariants (8), the row-offset
helper + step tuple (5), the §5.2 averaging-kernel correctness
including byte-parallel no-bleed verification (9) and an
inter-module row-stride cross-check linking `mc_kernel` to
`reconstruct::PREDICTOR_ROW_STRIDE` and
`mc_packed::MV_PIXEL_OFFSET_ROW_STRIDE` (1). Per the §5 chapter
boundary, the module surfaces the §5.1 / §5.2 / §5.3 kernel
shape only — not the cell-position decode of §5.4 (table-mediated
via `bank[+0x300]` / `bank[+0x700]`, values pending an Extractor
round per §7.5 and §8.2 item 4), not the §5.6 VQ-residual-after-MC
chain (the spec/06 entry at `IR32_32.DLL!0x10006bac`), and not
the §4.4 source-pointer bounds check (per spec the binary itself
does not range-check).

**Round 13 — Indeo 3 (IV31 / IV32) packed-MV bit-layout decode +
four-way MC dispatch (`spec/05` §2.2 / §2.3 / §3.3 / §3.4).**
Round 13 adds the `indeo3::mc_packed` module, the next slice in the
MC pipeline after round 12's table-layout surface: given the
already-fetched 32-bit packed-MV word, decompose it into the signed
strip-pixel byte offset and the half-pel filter-mode selector the
dispatcher branches on. [`PackedMv::from_raw`] wraps the DWORD;
[`PackedMv::pixel_offset`] recovers the §2.3 / §3.4 signed pixel
byte offset via the dispatcher's `sar edx, 0x2` at
`IR32_32.DLL!0x100066f3` ([`MV_PIXEL_OFFSET_SHIFT`] = `2`);
[`PackedMv::mode`] returns [`McDispatchMode`], the §2.2 four-way
fork (`FullPel` / `VerticalHalfPel` / `HorizontalHalfPel` /
`BothHalfPel`) selected by the `test edx, 0x1; test edx, 0x2` chain
at `0x100066e0..0x100066ee`, with each variant carrying its
inner-loop RVA (`0x1000670d` / `0x10006780` / `0x1000684b` /
`0x100068f8`). The §3.4 low-two-bit field labels are surfaced as
[`MV_VERT_HALFPEL_BIT`] (`0x1`), [`MV_HORIZ_HALFPEL_BIT`] (`0x2`),
and [`MV_MODE_BITS_MASK`] (`0x3`); the §3.3 row-stride constant
[`MV_PIXEL_OFFSET_ROW_STRIDE`] (`176` / `0xb0`) is aliased to
[`PREDICTOR_ROW_STRIDE`] with a `const _` cross-check enforcing the
two reconstructions agree. [`apply_mv_source_offset`] /
[`PackedMv::source_address`] model the §2.3
`src_addr = dst_cell_base + sign_extend(packed_MV >> 2)` (returning
`None` on signed underflow as a safe-Rust safety net — per §4.4 the
binary itself performs no bounds check). [`pack_mv_components`] is
the constructive inverse, surfacing the §3.3 closing-arithmetic
write `((176*vert + horiz) << 2) | (horiz_lsb << 1) | vert_lsb` so
round-trip tests can build a DWORD from `(vert, horiz, vert_lsb,
horiz_lsb)` directly. 20 new unit tests cover the §3.4 mode-bit
field disjointness + shift width (3), the §2.2 four-way dispatch
including the bits-outside-mask invariance and inner-loop-RVA
uniqueness (7), the §2.3 sign-extending source-pointer arithmetic
including signed underflow (4), and the `pack_mv_components`
round-trip across representative `(vert, horiz)` and all four
mode-bit pairs (6). Per the §3 / §5 chapter boundary, round 13
lands the decode + dispatch surface only — not the §5.1 / §5.2 /
§5.3 cell copy (per-row byte-pair averaging filter, `0xb0`-stride
destination walk), not the §3.3 `(vert, horiz)` re-decomposition
(the dispatcher uses the combined offset directly per §2.3 and the
spec does not pin down a division convention for the pair), and not
the bounds-check against the strip-buffer arena (per §4.4 the
binary has no such check).

**Round 12 — Indeo 3 (IV31 / IV32) per-plane packed-MV table
layout and INTER-leaf indexing surface (`spec/05` §1).**
Round 12 adds the `indeo3::mc_table` module, the per-plane
packed-MV table the binary writes during the picture-layer
prelude and reads at every MC_TREE INTER leaf. The arena occupies
the first 1024 bytes of the inner-instance state
([`MV_TABLE_BASE_OFFSET`] = `0x000`,
[`MV_TABLE_BYTES`] = `0x400`,
[`MV_TABLE_ENTRY_SIZE`] = `4`); the one-byte MV index fixes the
addressable maximum at 256 entries
([`MV_TABLE_MAX_BYTE_INDEXABLE_ENTRIES`]), with
[`mv_table_entry_byte_offset`] enforcing the bound and
[`MV_INDEX_SCALE_SHIFT`] (`2`) matching the §1.3
`shl eax, 0x2` scale step. The §1.2 four-way parser-arm dispatch
is surfaced as [`MvTableParserArm::from_frame_flags`]
(`FullPel` / `HalfPelHorizontal` / `HalfPelVertical` /
`HalfPelBoth`, masking on
[`MV_HALFPEL_HORIZ`] | [`MV_HALFPEL_VERT`] = `MV_HALFPEL_MASK`),
each variant carrying its `[ecx + 4*edx]` write-site RVA
(`0x10004572` / `0x10004493` / `0x10004510` / `0x10004426`) and
its per-component half-pel `<<= 1` disposition. The §1.3 INTER-
leaf sequence
(`xor eax,eax; mov al,[ebp]; shl eax,0x2; add eax,inner_instance`)
is modelled by [`MvIndexFetch::for_index`], which composes the
MV-index byte, table byte offset, parser arm, and §1.4 validity
classification ([`MvIndexValidity`]: `WrittenThisFrame` /
`StaleTailEntry` / `OutOfRange`) into a single descriptor — up
to but not including the table dereference itself, which is the
§3 packed-MV-decode chapter's subject. 27 new unit tests cover
the arena layout constants (5), the four-way parser-arm dispatch
including the bits-outside-mask invariance check and write-site
RVA uniqueness (7), the per-entry byte-offset helper across the
full 256-entry range (4), the §1.4 validity classifier across
WrittenThisFrame / StaleTailEntry / OutOfRange and the
`num_vectors > 256` corner case (6), and the
[`MvIndexFetch::for_index`] descriptor's helper-agreement /
parser-arm-tracking integration (5). Per the §1 chapter boundary,
round 12 lands the table-layout / index-arithmetic surface only —
not the packed-MV bit-layout decode (§3 — bottom 2 bits filter
mode, upper 30 bits signed strip-pixel byte offset), not the
four-way MC fetcher dispatch (§5.1 / §5.2 / §5.3), and not the
half-pel byte-pair averaging filter (§5.2).

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
| spec/05 §2.2 four-way MC dispatch on packed-MV `bits 1..0` | yes (`McDispatchMode`, `PackedMv::mode`) |
| spec/05 §2.3 source-pointer `add esi, sar(packed_mv, 2)` | yes (`apply_mv_source_offset`, `PackedMv::source_address`) |
| spec/05 §3.3 packing formula `176 * vert + horiz`        | yes (`pack_mv_components`, `MV_PIXEL_OFFSET_ROW_STRIDE`) |
| spec/05 §3.4 packed-MV byte layout (`bits 31..2`/`bit 1`/`bit 0`) | yes (`PackedMv`, `MV_VERT_HALFPEL_BIT`, `MV_HORIZ_HALFPEL_BIT`, `MV_MODE_BITS_MASK`, `MV_PIXEL_OFFSET_SHIFT`) |
| spec/05 §4.2 `frame_flags` bit 9 source / destination slot inversion (`0x100045b1..0x100045fd`) | yes (`Bank::from_buffer_selector`, `McBankAssignment::resolve`, `BANK_INVERSION_DELTA`) |
| spec/05 §4.3 source-pointer plumbing (`0x10006638..0x10006641`) | yes (`DecoderStackArg`, `DispatcherScratch`, `SourcePlumbingPair`, `DECODER_ARG_SRC_SLOT_OFFSET`, `DECODER_ARG_DST_SLOT_OFFSET`, `DISPATCHER_SCRATCH_SRC_DATA_OFFSET`, `DISPATCHER_SCRATCH_DST_DATA_OFFSET`, `DISPATCHER_SCRATCH_EXTRA_OFFSET_OFFSET`, `STRIP_CTX_ARRAY_ELEMENT_SHIFT`, `is_self_copy_degenerate`) |
| spec/05 §4.4 "no explicit boundary check" disposition    | yes (`MC_NO_BOUNDARY_CHECK`, `SourcePointerBoundsCheck`, `MvSourceOffsetClass`, `mv_source_offset_in_strip_region`, `STRIP_REGION_LUMA_240_BYTES`, `STRIP_REGION_LUMA_240_FITS_IN_ARENA`, `PaddingPixelPreservation`) |
| spec/05 §5.1 / §5.2 / §5.3 cell-copy inner loop          | deferred (strip pixel-buffer surface) |

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
* `oxideav_indeo::indeo3::PackedMv` (`::from_raw`, `::pixel_offset`,
  `::mode`, `::vert_half_pel_bit`, `::horiz_half_pel_bit`,
  `::source_address`) — spec/05 §3.4 typed view over a 32-bit
  packed-MV DWORD as fetched from `inner_instance[4*i]`. The §2.3
  `sar 2` recovers the signed strip-pixel byte offset; the low two
  bits feed the §2.2 four-way dispatch.
* `McDispatchMode` (`FullPel` / `VerticalHalfPel` /
  `HorizontalHalfPel` / `BothHalfPel`, `::from_packed_mv`,
  `::mode_bits`, `::inner_loop_rva`, `::applies_vertical_half_pel`,
  `::applies_horizontal_half_pel`, `::is_half_pel`) — spec/05 §2.2
  four-way MC dispatch on the packed-MV's bottom two bits, each
  variant carrying its inner-loop RVA.
* `apply_mv_source_offset(dst_cell_base, offset) -> Option<usize>`
  — spec/05 §2.3 sign-extending `add esi, edx`, returning `None`
  on signed underflow.
* `pack_mv_components(vert, horiz, vert_lsb, horiz_lsb) -> u32` —
  spec/05 §3.3 constructive packer
  (`((176*vert + horiz) << 2) | (horiz_lsb << 1) | vert_lsb`).
* Packed-MV constants: `MV_VERT_HALFPEL_BIT` (0x1),
  `MV_HORIZ_HALFPEL_BIT` (0x2), `MV_MODE_BITS_MASK` (0x3),
  `MV_PIXEL_OFFSET_SHIFT` (2), `MV_PIXEL_OFFSET_ROW_STRIDE` (176).
* `oxideav_indeo::indeo3::Bank` (`Primary` / `Secondary`,
  `::from_buffer_selector`, `::opposite`, `::slot_for_plane`,
  `::is_primary`, `::is_secondary`) — spec/05 §4.2 typed bank
  enum decoding `frame_flags` bit 9.
* `oxideav_indeo::indeo3::McBankAssignment::resolve(flags,
  plane_idx) -> Option<McBankAssignment>` — spec/05 §4.2 ping-pong
  resolver returning the `(dst_slot, src_slot, dst_bank)` triple
  the per-plane decoder pushes as `[esp+0x58]` / `[esp+0x54]`.
  `::src_bank()`, `::is_self_copy()` (`false` for well-formed
  results), `::slot_delta()` (identically `BANK_INVERSION_DELTA`).
* `BANK_INVERSION_DELTA` (`= 3`) — the spec/05 §4.2
  `PRIMARY_BANK_SLOTS[i] - SECONDARY_BANK_SLOTS[i]` identity.
* `oxideav_indeo::indeo3::DecoderStackArg` (`SrcSlot` / `DstSlot`,
  `::byte_offset`, `::role`, `::dispatcher_scratch`) — spec/05 §4.3
  typed pick of one of the two per-plane decoder stack-frame
  arguments at `[esp+0x54]` (source slot) / `[esp+0x58]`
  (destination slot).
* `oxideav_indeo::indeo3::DispatcherScratch` (`SrcCellData` /
  `DstCellData` / `ExtraOffset`, `::byte_offset`, `::role`,
  `::is_source_companion`) — spec/05 §4.3 / §7.2 typed pick of one
  of the three cell-state dispatcher scratch slots at `[esp+0x24]`
  / `[esp+0x28]` / `[esp+0x38]`.
* `oxideav_indeo::indeo3::SourcePlumbingPair::for_role(role)` —
  spec/05 §4.3 typed `(decoder_arg, dispatcher_scratch)` pair,
  with `::decoder_arg()`, `::dispatcher_scratch()`, `::role()`.
* `oxideav_indeo::indeo3::is_self_copy_degenerate(dst_slot,
  src_slot) -> bool` — spec/05 §4.3 closing predicate
  (`dst_slot == src_slot` ⇒ self-copy).
* Source-pointer-plumbing constants: `DECODER_ARG_SRC_SLOT_OFFSET`
  (`0x54`), `DECODER_ARG_DST_SLOT_OFFSET` (`0x58`),
  `DISPATCHER_SCRATCH_SRC_DATA_OFFSET` (`0x24`),
  `DISPATCHER_SCRATCH_DST_DATA_OFFSET` (`0x28`),
  `DISPATCHER_SCRATCH_EXTRA_OFFSET_OFFSET` (`0x38`),
  `STRIP_CTX_ARRAY_ELEMENT_SHIFT` (`2`).
* `oxideav_indeo::indeo3::MC_NO_BOUNDARY_CHECK` (`= true`) —
  spec/05 §4.4 paragraph 1 disposition flag: the binary performs
  no bounds check on the §2.3 source-pointer arithmetic.
* `oxideav_indeo::indeo3::SourcePointerBoundsCheck`
  (`BinaryDoesNotCheck` / `CallerOptsIn`, `::is_binary_path`,
  `::is_caller_opts_in`) — spec/05 §4.4 typed call-site
  disposition.
* `oxideav_indeo::indeo3::MvSourceOffsetClass` (`InRegion` /
  `OutOfRegion` / `Underflow`, `::is_in_region`,
  `::is_out_of_region`, `::is_underflow`, `::is_out_of_bounds`) —
  spec/05 §4.4 per-call classification of a source-pointer offset
  against a supplied strip region.
* `oxideav_indeo::indeo3::mv_source_offset_in_strip_region(
  dst_cell_base, mv_offset, strip_region_bytes_total) ->
  MvSourceOffsetClass` — spec/05 §4.4 paragraph 3 opt-in
  classifier; does not consume the §2.3 arithmetic itself.
* §4.4 worked-example constants:
  `STRIP_REGION_LUMA_240_BYTES` (`= 0xa500` = `42_240`, the §4.4
  paragraph 2 first-bullet `0xb0 * 240` figure),
  `STRIP_REGION_LUMA_240_FITS_IN_ARENA` (`= false`, the §4.1
  footnote discrepancy mirror — `0xa500 > 0x8020`).
* `oxideav_indeo::indeo3::PaddingPixelPreservation`
  (`DeterministicAtCodecInit` /
  `PreservedAcrossFramesByStripEdgeFixup`, `::is_codec_init`,
  `::is_frame_to_frame`) — spec/05 §4.4 paragraph 2 second-bullet
  typed disposition of the strip allocator's deterministic-pattern
  init vs the §5.4 strip-edge fix-up's frame-to-frame
  preservation.
* Constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `FLAG_YVU9_8BIT`, `NULL_FRAME_DATA_SIZE_BITS`, `MIN_DIMENSION`,
  `MAX_WIDTH`, `MAX_HEIGHT`, `PLANE_COUNT`, `PLANE_IDX_U`,
  `PLANE_IDX_V`, `PLANE_IDX_Y`, `NUM_VECTORS_FIELD_LEN`,
  `MC_VECTOR_ENTRY_LEN`, `MIN_PRELUDE_LEN`.
* `alt_quant_indices(byte) -> (primary, secondary)` for §3.9.

## License

MIT.
