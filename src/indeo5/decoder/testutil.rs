//! A bitstream writer for tests: frames built field by field from
//! `spec/01` to `spec/05` of the staged specification, with the layout
//! the decoder reads (see "Where the decoder follows the fixtures" in
//! the module docs).

use super::band::RvMap;
use super::huff::{BLOCK_PRESETS, MB_PRESETS};

/// Writes fields least significant bit first, the way the decoder reads
/// them (spec/00 §3).
pub(crate) struct BitWriter {
    pub bytes: Vec<u8>,
    bit: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        BitWriter {
            bytes: Vec::new(),
            bit: 0,
        }
    }

    pub fn put(&mut self, value: u32, n: u32) {
        for i in 0..n {
            if self.bit.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let b = (value >> i) & 1;
            *self.bytes.last_mut().unwrap() |= (b as u8) << (self.bit % 8);
            self.bit += 1;
        }
    }

    pub fn align(&mut self) {
        self.bit = self.bit.next_multiple_of(8);
    }

    /// Write symbol `s` of the code described by `rows`: `i` ones, a
    /// zero unless the row is the last, the row's width in bits.
    pub fn sym(&mut self, rows: &[u8], s: u32) {
        let mut base = 0;
        for (i, &w) in rows.iter().enumerate() {
            let size = 1u32 << w;
            if s < base + size {
                let last = i == rows.len() - 1;
                self.put((1 << i) - 1, i as u32);
                if !last {
                    self.put(0, 1);
                }
                let x = s - base;
                for j in (0..w as u32).rev() {
                    self.put((x >> j) & 1, 1);
                }
                return;
            }
            base += size;
        }
        panic!("symbol {s} not in the code");
    }

    /// Write the signed value `v` (-127..=127) as a macroblock-layer
    /// symbol of the default macroblock code.
    pub fn signed(&mut self, v: i32) {
        let s = if v > 0 { 2 * v - 1 } else { -2 * v };
        self.sym(MB_PRESETS[7], s as u32);
    }

    /// Patch a 24-bit little-endian field at byte `at`.
    pub fn patch24(&mut self, at: usize, value: usize) {
        self.bytes[at..at + 3].copy_from_slice(&(value as u32).to_le_bytes()[..3]);
    }
}

/// The level index of signed level `k` (spec/05 §2.2: odd for positive
/// levels, even for negative ones).
pub(crate) fn level_index(k: i32) -> u32 {
    assert_ne!(k, 0);
    if k > 0 {
        (2 * k - 1) as u32
    } else {
        (-2 * k) as u32
    }
}

/// A GOP header to write (spec/02 §1), always with a custom size.
pub(crate) struct Gop {
    pub flags: u32,
    /// `decomp_levels`: 0 for one luma band, 1 for four.
    pub decomp: u32,
    pub width: u32,
    pub height: u32,
    /// The 6-bit band info of every luma band, then of the chroma band.
    pub luma: Vec<u32>,
    pub chroma: u32,
}

impl Gop {
    /// One luma band of 16x16 macroblocks and 8x8 blocks, quarter-size
    /// chroma of 4x4 macroblocks and blocks.
    pub fn simple(width: u32, height: u32) -> Gop {
        Gop {
            flags: 0,
            decomp: 0,
            width,
            height,
            luma: vec![0],
            chroma: 6,
        }
    }
}

/// Picture start (spec/01 §3) and, with `gop`, the GOP header up to the
/// align before the frame header.
pub(crate) fn write_start(w: &mut BitWriter, ftype: u32, number: u32, gop: Option<&Gop>) {
    w.put(0x1f, 5);
    w.put(ftype, 3);
    w.put(number, 8);
    let Some(g) = gop else { return };
    w.put(g.flags, 8);
    w.put(g.decomp, 3);
    w.put(15, 4);
    // Height first, then width.
    w.put(g.height, 13);
    w.put(g.width, 13);
    for &info in &g.luma {
        w.put(info, 6);
    }
    w.put(g.chroma, 6);
    if g.flags & 8 != 0 {
        w.put(0, 4);
    }
    w.align();
    w.put(2 << 19, 24);
    w.align();
}

/// Frame header (spec/02 §2) with `flags` (bits 0, 4, 5 and 6 must be
/// clear: no optional field is written) and the default macroblock code.
pub(crate) fn write_frame_header(w: &mut BitWriter, flags: u32) {
    assert_eq!(flags & 0x71, 0);
    w.put(flags, 8);
    w.put(0, 3);
    w.align();
}

/// One macroblock to write.
#[derive(Clone, Default)]
pub(crate) struct Mb {
    /// Not coded: the header is the one flag bit.
    pub skip: bool,
    /// The type bit of an inter frame (band not inheriting): inter.
    pub inter: bool,
    /// One bit per block, block 0 in bit 0.
    pub cbp: u32,
    /// Quantiser delta, for a band that reads them.
    pub delta: i32,
    /// Vector deltas (dy, dx) against the previous macroblock's vector.
    pub mv: (i32, i32),
    /// For each coded block in order, its (run, level) pairs: `run` is
    /// the advance in scan positions (1 for the next one), `level` the
    /// signed level `k`.
    pub blocks: Vec<Vec<(u32, i32)>>,
}

/// One band to write: a band header and one coded tile with an explicit
/// size.
pub(crate) struct Band {
    /// band_flags bits 1 (inherit), 2 and 3 (quantiser deltas) and 6
    /// (`rv` is written).
    pub flags: u32,
    pub gq: u32,
    /// Run/value slot, written when flags bit 6 is set.
    pub rv: u32,
    /// Macroblocks of four blocks (macroblock size differs from the
    /// block size).
    pub four: bool,
    pub mbs: Vec<Mb>,
}

impl Band {
    pub fn new(four: bool, mbs: Vec<Mb>) -> Band {
        Band {
            flags: 0,
            gq: 0,
            rv: 8,
            four,
            mbs,
        }
    }
}

/// An empty band (spec/02 §3.3).
pub(crate) fn write_empty_band(w: &mut BitWriter) {
    w.put(1, 8);
}

/// Write `band` for a frame of type `ftype` with `frame_flags` (bit 7:
/// band sizes; bit 3: a delta for every macroblock of luma band 0).
/// `first_luma` says whether this is luma band 0.
pub(crate) fn write_band(
    w: &mut BitWriter,
    band: &Band,
    ftype: u32,
    frame_flags: u32,
    first_luma: bool,
) {
    let rv = RvMap::new(band.rv as usize, &[]).unwrap();
    let blk = BLOCK_PRESETS[7];
    let inherit = band.flags & 2 != 0;
    let reads_delta = band.flags & 0xc == 4;
    let delta_always = first_luma && frame_flags & 8 != 0;
    let band_start = w.bytes.len();
    assert_eq!(band.flags & !0x4e, 0);
    w.put(band.flags, 8);
    if frame_flags & 0x80 != 0 {
        w.put(0, 24);
    }
    if band.flags & 0x40 != 0 {
        w.put(band.rv, 3);
    }
    w.put(0, 1);
    w.put(band.gq, 5);
    w.align();
    // Tile header (spec/03 §2): coded, explicit 24-bit size.
    let tile_start = w.bytes.len();
    w.put(0, 1);
    w.put(1, 1);
    w.put(0xff, 8);
    w.put(0, 24);
    w.align();
    // Macroblock headers (spec/03 §4).
    for mb in &band.mbs {
        if mb.skip {
            w.put(1, 1);
            if reads_delta && delta_always {
                w.signed(mb.delta);
            }
            continue;
        }
        w.put(0, 1);
        if ftype != 0 && !inherit {
            w.put(mb.inter as u32, 1);
        }
        w.put(mb.cbp, if band.four { 4 } else { 1 });
        if reads_delta && (mb.cbp != 0 || delta_always) {
            w.signed(mb.delta);
        }
        if mb.inter && ftype != 0 && !inherit {
            w.signed(mb.mv.0);
            w.signed(mb.mv.1);
        }
    }
    w.align();
    // Block data (spec/05).
    for mb in &band.mbs {
        assert_eq!(mb.blocks.len(), mb.cbp.count_ones() as usize);
        for block in &mb.blocks {
            for &(run, level) in block {
                let code = level_index(level);
                match rv.symbol_for(run as u8, code as u16) {
                    Some(s) => w.sym(blk, s),
                    None => {
                        // Escape: run - 1, then the level index in two
                        // 6-bit halves, low first (spec/04 §5.5).
                        w.sym(blk, rv.esc());
                        w.sym(blk, run - 1);
                        w.sym(blk, code & 63);
                        w.sym(blk, code >> 6);
                    }
                }
            }
            w.sym(blk, rv.eob());
        }
    }
    w.align();
    let tile_size = w.bytes.len() - tile_start;
    // The 24-bit size starts 10 bits into the tile.
    let mut field = BitWriter::new();
    field.put(0, 1);
    field.put(1, 1);
    field.put(0xff, 8);
    field.put(tile_size as u32, 24);
    w.bytes[tile_start..tile_start + field.bytes.len()].copy_from_slice(&field.bytes);
    if frame_flags & 0x80 != 0 {
        w.patch24(band_start + 1, w.bytes.len() - band_start);
    }
}

/// A whole frame: picture start, GOP header for `gop`, frame header with
/// `frame_flags`, the luma bands, then the V and U bands (`None` writes
/// an empty band).
pub(crate) fn frame(
    ftype: u32,
    number: u32,
    gop: Option<&Gop>,
    frame_flags: u32,
    luma: &[Option<&Band>],
    v: Option<&Band>,
    u: Option<&Band>,
) -> Vec<u8> {
    let mut w = BitWriter::new();
    write_start(&mut w, ftype, number, gop);
    write_frame_header(&mut w, frame_flags);
    for (i, band) in luma.iter().enumerate() {
        match band {
            Some(b) => write_band(&mut w, b, ftype, frame_flags, i == 0),
            None => write_empty_band(&mut w),
        }
    }
    for band in [v, u] {
        match band {
            Some(b) => write_band(&mut w, b, ftype, frame_flags, false),
            None => write_empty_band(&mut w),
        }
    }
    w.bytes
}
