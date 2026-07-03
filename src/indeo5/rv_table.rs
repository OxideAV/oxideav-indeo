//! Indeo 5 per-band run-value (rv) table mechanism (`spec/05 §2`).
//!
//! Spec source: `docs/video/indeo/indeo5/spec/05-coefficient-decode.md`
//! §2 and §4.2.
//!
//! The per-block Huffman decode (`spec/05 §2.1`) yields a `vlc`
//! symbol which is **not yet** the run / level fields — a second
//! indirection through the band's rv-table maps it to a
//! `(run_add, lindex)` pair (`spec/05 §2.2`). The two byte streams
//! are stored as **parallel arrays** sharing one index space (the
//! `[esp+0x20]` / `[esp+0xfc]` base-pointer pair at
//! `IR50_32.DLL!0x1001f6a5..0x1001f6b7`). `lindex` then converts to
//! the signed quantised coefficient through the shared level zig-zag
//! table (`spec/05 §2.3`, [`super::build_level_table`]).
//!
//! The band header may patch the active table in place before decode
//! (`spec/05 §2.4`): each `(corr_index, corr_value)` pair from
//! `rv_tab_corr` (`spec/02 §3.4`, `<= 61` pairs) indexes the linear
//! concatenation of the two sub-arrays — an even `corr_index` patches
//! `run_add[corr_index / 2]`, an odd one patches
//! `lindex[corr_index / 2]`. The patch is destructive and per-band.
//!
//! **Docs-gap (spec/05 §7 items 1/2/8):** the *contents* of the eight
//! preset rv-tables (`rv_tab_sel = 0..7`), the ninth default slot
//! (`rv_tab_sel == 8`), and the sibling per-symbol bit-length arrays
//! have not been extracted — they are runtime-built in the
//! per-instance arena at `[band+0x1aeb8] + rv_tab_sel * 0x14`. This
//! module implements the *mechanism* over caller-supplied contents
//! and stops there.

use super::level_table::{level_value, LEVEL_TABLE_LEN};

/// Spec/05 §2.2 / §7 item 2 — the per-slot stride of the rv-table
/// arena (`[band+0x1aeb8] + rv_tab_sel * 0x14`).
pub const RV_TABLE_SLOT_STRIDE: u32 = 0x14;

/// Spec/05 §4.2 — the escape path's `lindex_hi` shift: the aggregated
/// index is `lindex_lo | (lindex_hi << 6)`, a 12-bit range.
pub const ESCAPE_LINDEX_HI_SHIFT: u32 = 6;

/// Errors raised by the rv-table mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RvTableError {
    /// The two parallel sub-arrays had different lengths.
    LengthMismatch {
        /// `run_add[]` length.
        run_add: usize,
        /// `lindex[]` length.
        lindex: usize,
    },
    /// A correction pair's `corr_index` addressed past the linear
    /// concatenation of the two sub-arrays (`spec/05 §2.4`).
    CorrectionOutOfRange {
        /// The offending `corr_index`.
        corr_index: u8,
        /// The table's entry count (per sub-array).
        entries: usize,
    },
}

impl core::fmt::Display for RvTableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RvTableError::LengthMismatch { run_add, lindex } => write!(
                f,
                "indeo5 rv-table: parallel arrays disagree ({run_add} run_add vs {lindex} lindex entries) (spec/05 §2.2)"
            ),
            RvTableError::CorrectionOutOfRange {
                corr_index,
                entries,
            } => write!(
                f,
                "indeo5 rv-table: correction index {corr_index} out of range for {entries} entries (spec/05 §2.4)"
            ),
        }
    }
}

impl std::error::Error for RvTableError {}

/// Spec/05 §2.2 — one band's active rv-table: the `(run_add[],
/// lindex[])` parallel byte arrays indexed by the decoded `vlc`
/// symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RvTable {
    run_add: Vec<u8>,
    lindex: Vec<u8>,
}

impl RvTable {
    /// Build from the two parallel sub-arrays (equal lengths
    /// required).
    pub fn new(run_add: Vec<u8>, lindex: Vec<u8>) -> Result<Self, RvTableError> {
        if run_add.len() != lindex.len() {
            return Err(RvTableError::LengthMismatch {
                run_add: run_add.len(),
                lindex: lindex.len(),
            });
        }
        Ok(RvTable { run_add, lindex })
    }

    /// Entries per sub-array (the shared `vlc` index space).
    pub fn len(&self) -> usize {
        self.run_add.len()
    }

    /// `true` when the table carries no entries.
    pub fn is_empty(&self) -> bool {
        self.run_add.is_empty()
    }

    /// Spec/05 §2.2 — look up the `(run_add, lindex)` pair for a
    /// decoded `vlc` symbol.
    pub fn lookup(&self, vlc: u8) -> Option<(u8, u8)> {
        let i = vlc as usize;
        if i < self.run_add.len() {
            Some((self.run_add[i], self.lindex[i]))
        } else {
            None
        }
    }

    /// Spec/05 §2.4 — apply the band header's `rv_tab_corr` patch
    /// pairs in order. Each `(corr_index, corr_value)` addresses the
    /// linear concatenation of the two sub-arrays: even indices patch
    /// `run_add[i/2]`, odd indices patch `lindex[i/2]`. Destructive
    /// in-place; the patched table is the one consumed for the rest
    /// of the band.
    pub fn apply_corrections(&mut self, pairs: &[(u8, u8)]) -> Result<(), RvTableError> {
        for &(corr_index, corr_value) in pairs {
            let slot = (corr_index / 2) as usize;
            if slot >= self.run_add.len() {
                return Err(RvTableError::CorrectionOutOfRange {
                    corr_index,
                    entries: self.run_add.len(),
                });
            }
            if corr_index % 2 == 0 {
                self.run_add[slot] = corr_value;
            } else {
                self.lindex[slot] = corr_value;
            }
        }
        Ok(())
    }
}

/// Spec/05 §4.2 — the escape path's `lindex` aggregation from the two
/// extra VLC symbols: `lindex = lindex_lo | (lindex_hi << 6)` (a
/// 12-bit range, vs the regular path's 6-bit range).
pub fn escape_lindex(lindex_lo: u8, lindex_hi: u8) -> u16 {
    u16::from(lindex_lo) | (u16::from(lindex_hi) << ESCAPE_LINDEX_HI_SHIFT)
}

/// Spec/05 §4.2 / §3.1 — the scan-position advance after one decoded
/// symbol: `run += run_add + 1` (the wiki's "next coefficient
/// position = previous + run + 1" identity).
pub fn run_advance(run: u32, run_add: u8) -> u32 {
    run + u32::from(run_add) + 1
}

/// Spec/05 §2.3 — convert a (regular-path, 6-bit-range) `lindex` to
/// the signed quantised coefficient via the level zig-zag table
/// (`coefficient = byte_ptr [lindex + 0x1009e438]`).
///
/// The wiki annex's `level_tables[run][lindex - 1]` re-numbering
/// (where `lindex == 0` means "no coefficient at this position") is
/// absorbed by the binary's per-handler arithmetic (`spec/05 §5`);
/// the raw lookup here is the §2.3 direct byte load.
pub fn coefficient_for_lindex(level_table: &[i8; LEVEL_TABLE_LEN], lindex: u8) -> i8 {
    level_value(level_table, lindex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indeo5::level_table::build_level_table;

    #[test]
    fn parallel_arrays_must_match() {
        assert!(matches!(
            RvTable::new(vec![1, 2], vec![3]),
            Err(RvTableError::LengthMismatch {
                run_add: 2,
                lindex: 1
            })
        ));
        let t = RvTable::new(vec![1, 2], vec![3, 4]).unwrap();
        assert_eq!(t.len(), 2);
        assert!(!t.is_empty());
    }

    #[test]
    fn lookup_pairs_share_index_space() {
        let t = RvTable::new(vec![0, 1, 2], vec![10, 11, 12]).unwrap();
        assert_eq!(t.lookup(0), Some((0, 10)));
        assert_eq!(t.lookup(2), Some((2, 12)));
        assert_eq!(t.lookup(3), None);
    }

    #[test]
    fn corrections_patch_interleaved() {
        // spec/05 §2.4 — even corr_index patches run_add, odd patches
        // lindex, both at slot corr_index/2.
        let mut t = RvTable::new(vec![0, 0, 0], vec![0, 0, 0]).unwrap();
        t.apply_corrections(&[(0, 5), (1, 6), (4, 7), (5, 8)])
            .unwrap();
        assert_eq!(t.lookup(0), Some((5, 6)));
        assert_eq!(t.lookup(1), Some((0, 0)));
        assert_eq!(t.lookup(2), Some((7, 8)));
    }

    #[test]
    fn corrections_are_destructive_and_ordered() {
        // A later pair overwrites an earlier one (in-place patching).
        let mut t = RvTable::new(vec![0], vec![0]).unwrap();
        t.apply_corrections(&[(0, 1), (0, 2)]).unwrap();
        assert_eq!(t.lookup(0), Some((2, 0)));
    }

    #[test]
    fn correction_out_of_range_rejected() {
        let mut t = RvTable::new(vec![0, 0], vec![0, 0]).unwrap();
        // corr_index 4 -> slot 2, past the 2-entry table.
        assert!(matches!(
            t.apply_corrections(&[(4, 9)]),
            Err(RvTableError::CorrectionOutOfRange {
                corr_index: 4,
                entries: 2
            })
        ));
    }

    #[test]
    fn escape_lindex_twelve_bit_range() {
        // spec/05 §4.2 — lindex = lo | (hi << 6).
        assert_eq!(escape_lindex(0, 0), 0);
        assert_eq!(escape_lindex(0x3f, 0), 0x3f);
        assert_eq!(escape_lindex(0, 1), 0x40);
        assert_eq!(escape_lindex(0x3f, 0x3f), 0xfff);
    }

    #[test]
    fn run_advance_identity() {
        // next position = previous + run_add + 1.
        assert_eq!(run_advance(0, 0), 1);
        assert_eq!(run_advance(5, 3), 9);
    }

    #[test]
    fn coefficient_via_level_table() {
        let table = build_level_table();
        // spec/04 §3.4 recurrence: index 0 -> -0x80, index 1 -> -0x7f,
        // index 2 -> +0x7f (the zig-zag fold).
        assert_eq!(coefficient_for_lindex(&table, 0), table[0]);
        assert_eq!(coefficient_for_lindex(&table, 1), table[1]);
    }

    #[test]
    fn slot_stride_constant() {
        assert_eq!(RV_TABLE_SLOT_STRIDE, 0x14);
    }
}
