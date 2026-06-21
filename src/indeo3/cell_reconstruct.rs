//! Indeo 3 static-table-only per-cell reconstruction executor.
//!
//! Spec source: `docs/video/indeo/indeo3/spec/06-entropy.md` §3 / §4
//! (the per-cell mode-byte stream + the `0xF8..0xFF` RLE escapes) and
//! `spec/07-output-reconstruction.md` §1 / §3 (the in-cell predictor
//! chain + the static `.data + 0x1003d088` dyad table the high-nibble-0
//! handler reads).
//!
//! ## What this module adds
//!
//! The earlier rounds landed every reconstruction sub-step as an
//! independent caller-driven primitive: [`super::apply_row_band_seed`]
//! (one high-nibble-0 static-table seed write), [`super::emit_cell_chain`]
//! (the predictor chain given *caller-supplied* dyad deltas),
//! [`super::ModeByte::classify`] (one mode-byte classification),
//! [`super::RleEscape::accepted_at`] (one escape's position acceptance).
//! None of them *consumed a mode-byte stream* and *drove the strip
//! buffer* end-to-end.
//!
//! This module owns the per-cell mode-byte **executor**: given a cell's
//! geometry and the slice of mode bytes the per-cell unpacker would read
//! from `[ebp]` (`spec/06 §1.2`), it walks the cell row by row, reads one
//! mode byte per position, and dispatches it to the handler the binary's
//! high-nibble jump table (`spec/06 §3.2`) would select:
//!
//! * **High-nibble-0 literal** (`spec/07 §3.1`/`§3.2`,
//!   `IR32_32.DLL!0x10006c14`) → reads the signed delta byte from the
//!   on-disk static dyad table at `.data + 0x1003d088` and writes it
//!   into the predictor slot one row above ([`super::apply_row_band_seed`]).
//!   This path needs **no per-frame arena**, so it is fully decodable
//!   from the vendored static table.
//! * **RLE skip escapes** `0xFD` / `0xFE` / `0xFF` (`spec/06 §4.2`) →
//!   advance the destination pointer by all-remaining / two / one row
//!   stride(s) without emitting deltas. No table input.
//! * **RLE escape `0xFB`** (`spec/06 §4.4`) → consumes the counter byte
//!   and reports the skip-cells category; it terminates the cell.
//! * **Position-faulting escapes** (`spec/06 §4.3`) → the §4.3
//!   acceptance matrix is enforced: an escape emitted at a position
//!   where the binary's per-position dispatch table routes to the fault
//!   handler `0x1000854b` aborts with [`CellReconstructError::EscapeFault`]
//!   (the binary's error-code-1 return).
//!
//! ## What it defers (the codebook-bank docs-gap)
//!
//! A literal mode byte whose **high nibble is non-zero** addresses the
//! per-frame VQ codebook arena (`spec/06 §3.1`: `low_nibble × 2048`
//! selects the band; the dyad-pair DWORD lives in the arena's primary /
//! secondary tables). Those arena values are built at codec-init by
//! `IR32_32.DLL!0x100060de` and are the `spec/04 §7.1` Extractor
//! docs-gap (zero on disk; the §5.2 seed-window materialisation is
//! blocked on the spec-vs-audit `0x1004d26a` block-format contradiction,
//! audit/00 §2.3). Rather than guess, the executor stops at the first
//! such byte and returns [`CellOutcome::DeferredArena`], surfacing
//! *exactly* which mode byte and position the docs-gap bites — the
//! cleanest possible boundary report for the next Extractor round.
//!
//! ## Scope
//!
//! This executor models the **variant-A row-0 plain-dyad** cell shape
//! (`spec/06 §3.4` variant A): one mode byte per dyad-pair position,
//! `source_rows` rows of `width_dwords` positions each, with the §3.2
//! `0x10006bd4` jump-table dispatch. The averaging variants (B/C/D) and
//! the variable-byte continuation (`spec/06 §3.3`) are out of scope for
//! this round because they consume the arena dyad values this module
//! defers; the executor's mission is the *static-table-only* subset.

use super::entropy::{ModeByte, ModeByteKind, PositionClass, RleEscape};
use super::reconstruct::PREDICTOR_ROW_STRIDE;
use super::vq::{apply_row_band_seed, DyadDeltaTable};

/// Geometry of the cell the executor walks.
///
/// Mirrors [`super::CellEmitGeometry`] but for the static-table-only
/// path: a cell is `source_rows` rows tall and `width_dwords` dyad-pair
/// positions wide, with its top-left pixel at `top_left_offset` in the
/// strip pixel buffer (row stride [`PREDICTOR_ROW_STRIDE`] = `0xb0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellReconstructGeometry {
    /// Cell width in dyad-pair positions (pixels / 4); each position
    /// reads one mode byte (`spec/06 §3.4`).
    pub width_dwords: usize,
    /// Number of rows the cell spans (`spec/07 §1.2` `N ∈ {4, 8}`).
    pub source_rows: usize,
    /// Byte offset of the cell's top-left pixel within the strip buffer.
    pub top_left_offset: usize,
}

/// One mode byte's effect, recorded for the per-cell trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEffect {
    /// A high-nibble-0 literal: the static dyad table was read and a
    /// signed delta byte was written into the predictor slot one row
    /// above (`spec/07 §3.2`). Carries the [`super::RowBandSeed`]'s
    /// signed delta and the slot it wrote.
    RowBandSeed {
        /// The signed delta byte read from `.data + 0x1003d088`.
        delta: i8,
        /// The predictor-slot byte index that was written
        /// (`write_index - 0xb0`).
        predictor_slot: usize,
    },
    /// An RLE skip escape (`0xFD` / `0xFE` / `0xFF`) advanced the
    /// destination pointer without emitting deltas. Carries how many
    /// row strides were skipped.
    RowSkip {
        /// The escape that triggered the skip.
        escape: RleEscape,
        /// Number of `0xb0`-byte row strides skipped.
        rows_skipped: usize,
    },
}

/// How a cell's mode-byte walk finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellOutcome {
    /// Every position resolved through a static-table-only handler; the
    /// cell is fully reconstructed in the strip buffer. Carries the
    /// per-position effect trace in row-major order.
    Complete(Vec<PositionEffect>),
    /// The walk reached a literal mode byte whose **high nibble is
    /// non-zero** — an arena-gated dyad whose value is the `spec/04
    /// §7.1` codebook-bank docs-gap. The walk stops *before* the byte;
    /// the partial trace up to it is carried so callers can see how far
    /// the static-table-only path reached.
    DeferredArena {
        /// The arena-gated mode byte the walk stopped at.
        mode_byte: u8,
        /// Its non-zero high nibble (`mode_byte >> 4`).
        high_nibble: u8,
        /// The source-row index (0-based) of the deferred position.
        row: usize,
        /// The dyad-pair column index (0-based) of the deferred
        /// position.
        dword: usize,
        /// The effects emitted before the deferral (row-major).
        emitted: Vec<PositionEffect>,
    },
    /// The walk hit an `0xFB` cell-terminating escape (`spec/06 §4.4`):
    /// the counter byte was consumed and the cell ends here. Carries the
    /// raw counter byte plus the effects emitted before it.
    Terminated {
        /// The raw `0xFB` counter byte read at `[ebp + 1]`.
        counter: u8,
        /// The effects emitted before the terminator (row-major).
        emitted: Vec<PositionEffect>,
    },
}

/// Errors the static-table-only executor surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellReconstructError {
    /// `width_dwords` or `source_rows` is zero — a degenerate cell.
    ZeroDimension {
        /// `true` if `width_dwords == 0`, `false` if `source_rows == 0`.
        is_width: bool,
    },
    /// The mode-byte slice was exhausted before the walk reached a
    /// terminator or completed every position. Carries the byte index
    /// the next read would have used and the slice length.
    ByteStreamExhausted {
        /// The byte index the next read would have used.
        next_index: usize,
        /// The supplied mode-byte slice length.
        supplied: usize,
    },
    /// An RLE escape was emitted at a position where the `spec/06 §4.3`
    /// per-position acceptance matrix routes it to the fault handler
    /// `0x1000854b` (the binary's error-code-1 return).
    EscapeFault {
        /// The faulting escape byte.
        escape: u8,
        /// The position class at which it faulted.
        position: PositionClass,
        /// The source-row index (0-based) of the faulting position.
        row: usize,
        /// The dyad-pair column index (0-based) of the faulting
        /// position.
        dword: usize,
    },
    /// A high-nibble-0 row-band seed write fell outside the strip buffer
    /// or above the strip top (no row above to seed; `spec/07 §1.3`).
    /// Carries the write index the seed targeted.
    SeedWriteOutOfBounds {
        /// The dyad-pair write index whose `- 0xb0` predictor slot was
        /// out of range.
        write_index: usize,
        /// The strip buffer length.
        buffer_len: usize,
    },
}

impl core::fmt::Display for CellReconstructError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CellReconstructError::ZeroDimension { is_width } => write!(
                f,
                "spec/07 §1.2: cell {} is zero",
                if *is_width {
                    "width"
                } else {
                    "source-row count"
                }
            ),
            CellReconstructError::ByteStreamExhausted {
                next_index,
                supplied,
            } => write!(
                f,
                "spec/06 §1.2: mode-byte stream exhausted at index {next_index} (len {supplied})"
            ),
            CellReconstructError::EscapeFault {
                escape,
                position,
                row,
                dword,
            } => write!(
                f,
                "spec/06 §4.3: escape {escape:#04x} faults (error code 1) at {position:?} \
                 (row {row}, dword {dword})"
            ),
            CellReconstructError::SeedWriteOutOfBounds {
                write_index,
                buffer_len,
            } => write!(
                f,
                "spec/07 §3.2: row-band seed at write index {write_index} is outside the \
                 {buffer_len}-byte strip buffer"
            ),
        }
    }
}

impl std::error::Error for CellReconstructError {}

/// Classify the position within a cell row for the `spec/06 §4.3`
/// escape-acceptance matrix.
///
/// Position 0 of any row is the row's first position; row 0 position 0
/// is additionally the cell's first position. Subsequent positions are
/// continuations 1, 2, 3 (capped at 3 — the §4.3 tables only narrow
/// through three continuations).
fn position_class(row: usize, dword: usize) -> PositionClass {
    match dword {
        0 if row == 0 => PositionClass::CellFirst,
        0 => PositionClass::RowFirst,
        1 => PositionClass::Continuation1,
        2 => PositionClass::Continuation2,
        _ => PositionClass::Continuation3,
    }
}

/// Spec/06 §3 / §4 + spec/07 §1 / §3 — drive one cell through the
/// static-table-only mode-byte executor.
///
/// Walks the cell's `source_rows` rows top to bottom; for each row walks
/// the `width_dwords` dyad-pair positions left to right (`spec/07 §2.4`
/// row-major order). At each position it reads one mode byte from
/// `mode_bytes[cursor]` (advancing `cursor`) and dispatches:
///
/// * **literal, high nibble 0** → [`apply_row_band_seed`] over `strip`
///   (the on-disk `0x1003d088` table; `spec/07 §3.2`). Records a
///   [`PositionEffect::RowBandSeed`].
/// * **literal, high nibble ≠ 0** → returns [`CellOutcome::DeferredArena`]
///   (the `spec/04 §7.1` codebook-bank docs-gap).
/// * **escape `0xFF` / `0xFE` / `0xFD`** → row skip (`spec/06 §4.2`);
///   records a [`PositionEffect::RowSkip`] and advances past the skipped
///   rows. `0xFD` ends the cell.
/// * **escape `0xFB`** → reads the counter byte and returns
///   [`CellOutcome::Terminated`] (`spec/06 §4.4`).
/// * **escape `0xF8` / `0xF9` / `0xFA` / `0xFC`** → checked against the
///   §4.3 acceptance matrix; accepted ones terminate or mark and end the
///   cell, position-rejected ones return [`CellReconstructError::EscapeFault`].
///
/// On success the `strip` buffer holds the cell's static-table-seeded
/// predictor bytes. Returns the [`CellOutcome`] describing how the walk
/// finished, or a typed [`CellReconstructError`].
pub fn reconstruct_cell_static(
    strip: &mut [u8],
    geometry: CellReconstructGeometry,
    mode_bytes: &[u8],
    table: &DyadDeltaTable,
) -> Result<CellOutcome, CellReconstructError> {
    if geometry.width_dwords == 0 {
        return Err(CellReconstructError::ZeroDimension { is_width: true });
    }
    if geometry.source_rows == 0 {
        return Err(CellReconstructError::ZeroDimension { is_width: false });
    }

    let mut effects: Vec<PositionEffect> = Vec::new();
    let mut cursor = 0usize;
    // The destination byte offset of the current row's first position
    // (`edi`); the §1.2 outer-loop tail advances it by a row stride per
    // emitted row.
    let mut row_dst_offset = geometry.top_left_offset;
    let mut row = 0usize;

    while row < geometry.source_rows {
        let mut dword = 0usize;
        // The number of extra row strides an `0xFE`/`0xFF` skip consumed
        // this iteration of the row loop; folded into the row advance.
        let mut extra_skip_rows = 0usize;

        while dword < geometry.width_dwords {
            let raw = read_byte(mode_bytes, &mut cursor)?;
            let mode = ModeByte::classify(raw);
            let position = position_class(row, dword);

            match mode.kind {
                ModeByteKind::Literal(lit) => {
                    if lit.high_nibble != 0 {
                        // Arena-gated dyad — the codebook-bank docs-gap.
                        return Ok(CellOutcome::DeferredArena {
                            mode_byte: raw,
                            high_nibble: lit.high_nibble,
                            row,
                            dword,
                            emitted: effects,
                        });
                    }
                    // High-nibble-0 row-band-advance: read the static
                    // dyad delta and seed the predictor slot above.
                    let write_index = row_dst_offset + dword * PIXELS_PER_DYAD_DWORD;
                    let seed =
                        apply_row_band_seed(table, strip, write_index, lit.high_nibble, row, dword)
                            .ok_or(CellReconstructError::SeedWriteOutOfBounds {
                                write_index,
                                buffer_len: strip.len(),
                            })?;
                    effects.push(PositionEffect::RowBandSeed {
                        delta: seed.delta,
                        predictor_slot: seed.predictor_slot,
                    });
                    dword += 1;
                }
                ModeByteKind::Escape(escape) => {
                    if !escape.accepted_at(position) {
                        return Err(CellReconstructError::EscapeFault {
                            escape: raw,
                            position,
                            row,
                            dword,
                        });
                    }
                    match escape {
                        // `0xFF` — skip line 1 of this block (one row).
                        RleEscape::Ff => {
                            effects.push(PositionEffect::RowSkip {
                                escape,
                                rows_skipped: 1,
                            });
                            extra_skip_rows = 1;
                            break;
                        }
                        // `0xFE` — skip lines 1 and 2 (two rows).
                        RleEscape::Fe => {
                            effects.push(PositionEffect::RowSkip {
                                escape,
                                rows_skipped: 2,
                            });
                            extra_skip_rows = 2;
                            break;
                        }
                        // `0xFD` — skip all remaining rows: the cell ends.
                        RleEscape::Fd => {
                            let remaining = geometry.source_rows - row;
                            effects.push(PositionEffect::RowSkip {
                                escape,
                                rows_skipped: remaining,
                            });
                            return Ok(CellOutcome::Complete(effects));
                        }
                        // `0xFB` — counter byte; the cell terminates.
                        RleEscape::Fb => {
                            let counter = read_byte(mode_bytes, &mut cursor)?;
                            return Ok(CellOutcome::Terminated {
                                counter,
                                emitted: effects,
                            });
                        }
                        // `0xFC` — skip the rest of this cell and the
                        // next (`spec/06 §4.2`); the current cell ends.
                        RleEscape::Fc => {
                            return Ok(CellOutcome::Complete(effects));
                        }
                        // `0xF8` / `0xF9` / `0xFA` — start-of-cell
                        // edge-mark family; accepted only at the cell's
                        // first position (the §4.3 matrix gate above
                        // already enforced that). They mark the cell and
                        // end the static-table walk: the boundary-marker
                        // semantics (`spec/07 §4.4`) are propagated by
                        // the caller's mark-edge path.
                        RleEscape::F8 | RleEscape::F9 | RleEscape::Fa => {
                            return Ok(CellOutcome::Complete(effects));
                        }
                    }
                }
            }
        }

        // §1.2 outer-loop tail: advance `edi` by the emitted row(s).
        // A normal row advances by one stride; an `0xFE`/`0xFF` skip
        // advanced by its skipped-row count instead.
        let advance_rows = if extra_skip_rows > 0 {
            extra_skip_rows
        } else {
            1
        };
        row_dst_offset += advance_rows * PREDICTOR_ROW_STRIDE;
        row += advance_rows;
    }

    Ok(CellOutcome::Complete(effects))
}

/// The number of pixels (= bytes) one dyad-pair position covers in the
/// strip buffer (`spec/07 §2.4`: a dyad-pair DWORD is 4 pixels).
const PIXELS_PER_DYAD_DWORD: usize = 4;

/// Read one byte from the mode-byte stream, advancing the cursor.
fn read_byte(bytes: &[u8], cursor: &mut usize) -> Result<u8, CellReconstructError> {
    match bytes.get(*cursor) {
        Some(&b) => {
            *cursor += 1;
            Ok(b)
        }
        None => Err(CellReconstructError::ByteStreamExhausted {
            next_index: *cursor,
            supplied: bytes.len(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIDE: usize = PREDICTOR_ROW_STRIDE;

    fn geom(width: usize, rows: usize, top: usize) -> CellReconstructGeometry {
        CellReconstructGeometry {
            width_dwords: width,
            source_rows: rows,
            top_left_offset: top,
        }
    }

    #[test]
    fn zero_dimensions_rejected() {
        let table = DyadDeltaTable::load();
        let mut strip = vec![0u8; STRIDE * 8];
        assert_eq!(
            reconstruct_cell_static(&mut strip, geom(0, 4, STRIDE), &[], &table),
            Err(CellReconstructError::ZeroDimension { is_width: true })
        );
        assert_eq!(
            reconstruct_cell_static(&mut strip, geom(1, 0, STRIDE), &[], &table),
            Err(CellReconstructError::ZeroDimension { is_width: false })
        );
    }

    #[test]
    fn exhausted_stream_is_typed_error() {
        let table = DyadDeltaTable::load();
        let mut strip = vec![0u8; STRIDE * 8];
        // 1×4 cell needs 4 bytes; supply 1.
        let err =
            reconstruct_cell_static(&mut strip, geom(1, 4, STRIDE), &[0x00], &table).unwrap_err();
        assert_eq!(
            err,
            CellReconstructError::ByteStreamExhausted {
                next_index: 1,
                supplied: 1,
            }
        );
    }

    #[test]
    fn high_nibble_zero_literals_seed_predictor_slots() {
        let table = DyadDeltaTable::load();
        // Cell starts at row 1 so every row-band seed has a row above.
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 6];
        // 1 dyad-pair wide × 4 rows: four high-nibble-0 literals.
        // Low nibble = column within the high-nibble-0 bank's row; we use
        // 0x00 at every position.
        let bytes = [0x00u8, 0x00, 0x00, 0x00];
        let outcome = reconstruct_cell_static(&mut strip, geom(1, 4, top), &bytes, &table).unwrap();
        match outcome {
            CellOutcome::Complete(effects) => {
                assert_eq!(effects.len(), 4);
                for (i, eff) in effects.iter().enumerate() {
                    match eff {
                        PositionEffect::RowBandSeed { predictor_slot, .. } => {
                            // Row k's write is at top + k*stride; its
                            // predictor slot is one stride above.
                            let write_index = top + i * STRIDE;
                            assert_eq!(*predictor_slot, write_index - STRIDE);
                            // The slot now holds the biased delta byte
                            // (non-trivially written).
                        }
                        other => panic!("unexpected effect {other:?}"),
                    }
                }
                // The first row's seed wrote into row 0 of the strip.
                assert_ne!(strip[0], 0x00, "row-band seed wrote a biased byte");
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn high_nibble_nonzero_literal_defers_to_arena() {
        let table = DyadDeltaTable::load();
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 6];
        // First byte 0x00 (static seed), second byte 0x30 (high nibble 3
        // → arena-gated). The walk should emit one seed, then defer.
        let bytes = [0x00u8, 0x30, 0x00, 0x00];
        let outcome = reconstruct_cell_static(&mut strip, geom(2, 2, top), &bytes, &table).unwrap();
        match outcome {
            CellOutcome::DeferredArena {
                mode_byte,
                high_nibble,
                row,
                dword,
                emitted,
            } => {
                assert_eq!(mode_byte, 0x30);
                assert_eq!(high_nibble, 3);
                assert_eq!(row, 0);
                assert_eq!(dword, 1);
                assert_eq!(emitted.len(), 1);
            }
            other => panic!("expected DeferredArena, got {other:?}"),
        }
    }

    #[test]
    fn ff_skips_one_row() {
        let table = DyadDeltaTable::load();
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 8];
        // Row 0: 0xFF at the row-first position skips one row.
        // Row 1: a static seed at dword 0, then the cell completes (1
        // wide). 0xFF is accepted at RowFirst.
        let bytes = [0xFFu8, 0x00];
        let outcome = reconstruct_cell_static(&mut strip, geom(1, 2, top), &bytes, &table).unwrap();
        match outcome {
            CellOutcome::Complete(effects) => {
                assert_eq!(effects.len(), 2);
                assert!(matches!(
                    effects[0],
                    PositionEffect::RowSkip {
                        escape: RleEscape::Ff,
                        rows_skipped: 1
                    }
                ));
                assert!(matches!(effects[1], PositionEffect::RowBandSeed { .. }));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn fd_skips_all_remaining_rows_and_completes() {
        let table = DyadDeltaTable::load();
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 8];
        // Row 0: a seed, then row 1 starts with 0xFD which skips the rest.
        let bytes = [0x00u8, 0xFD];
        let outcome = reconstruct_cell_static(&mut strip, geom(1, 4, top), &bytes, &table).unwrap();
        match outcome {
            CellOutcome::Complete(effects) => {
                // 1 seed (row 0) + 1 row-skip-all (row 1, 3 remaining).
                assert_eq!(effects.len(), 2);
                assert!(matches!(
                    effects[1],
                    PositionEffect::RowSkip {
                        escape: RleEscape::Fd,
                        rows_skipped: 3
                    }
                ));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn fb_reads_counter_and_terminates() {
        let table = DyadDeltaTable::load();
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 8];
        // 0xFB is accepted at the cell-first position; it reads a counter.
        let bytes = [0xFBu8, 0x25];
        let outcome = reconstruct_cell_static(&mut strip, geom(2, 4, top), &bytes, &table).unwrap();
        match outcome {
            CellOutcome::Terminated { counter, emitted } => {
                assert_eq!(counter, 0x25);
                assert!(emitted.is_empty());
            }
            other => panic!("expected Terminated, got {other:?}"),
        }
    }

    #[test]
    fn ff_mid_row_faults_per_acceptance_matrix() {
        let table = DyadDeltaTable::load();
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 8];
        // Width 2: position 0 is a seed, position 1 is Continuation1
        // where 0xFF faults (§4.3).
        let bytes = [0x00u8, 0xFF];
        let err = reconstruct_cell_static(&mut strip, geom(2, 2, top), &bytes, &table).unwrap_err();
        assert_eq!(
            err,
            CellReconstructError::EscapeFault {
                escape: 0xFF,
                position: PositionClass::Continuation1,
                row: 0,
                dword: 1,
            }
        );
    }

    #[test]
    fn f8_at_cell_start_is_accepted_and_completes() {
        let table = DyadDeltaTable::load();
        let top = STRIDE;
        let mut strip = vec![0u8; STRIDE * 8];
        // 0xF8 is start-of-cell only; at the first position it marks the
        // cell and ends the static-table walk.
        let bytes = [0xF8u8];
        let outcome = reconstruct_cell_static(&mut strip, geom(2, 4, top), &bytes, &table).unwrap();
        assert!(matches!(outcome, CellOutcome::Complete(_)));
    }

    #[test]
    fn position_class_maps_row_and_column() {
        assert_eq!(position_class(0, 0), PositionClass::CellFirst);
        assert_eq!(position_class(1, 0), PositionClass::RowFirst);
        assert_eq!(position_class(0, 1), PositionClass::Continuation1);
        assert_eq!(position_class(0, 2), PositionClass::Continuation2);
        assert_eq!(position_class(0, 3), PositionClass::Continuation3);
        assert_eq!(position_class(0, 9), PositionClass::Continuation3);
    }
}
