//! Helpers shared across the Indeo generations.
//!
//! Round 1 only ships the Indeo 2 decoder so this module is currently
//! light: a little-endian bit reader (used by the Indeo 2 entropy
//! payload — and likely re-usable by Indeo 3+, which all draw from
//! the same bit-pump idiom). As later generations land, shared VLC /
//! Huffman primitives and pixel clamps will gather here.

/// Little-endian bit reader, byte-aligned at construction.
///
/// Indeo 2's entropy payload (per
/// `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md` §3.2)
/// is read directly with a little-endian bit reader after the 48-byte
/// frame header. Bits are pulled MSB-first within each byte; the byte
/// stream is consumed left-to-right.
///
/// `pos_bits()` reports the current cursor in bits from the start of
/// the underlying buffer, which is exactly what `[TRACE/code]` lines
/// in the reference document carry — making the reader directly
/// auditable against the trace log.
#[derive(Debug)]
pub struct BitReader<'a> {
    buf: &'a [u8],
    /// Total bits consumed from `buf`.
    pos_bits: usize,
}

impl<'a> BitReader<'a> {
    /// Create a new reader positioned at bit 0 of `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos_bits: 0 }
    }

    /// Current cursor, in bits, from the start of the underlying
    /// buffer. Cheap to call.
    pub fn pos_bits(&self) -> usize {
        self.pos_bits
    }

    /// Total bits still available in the underlying buffer.
    pub fn bits_remaining(&self) -> usize {
        self.buf
            .len()
            .saturating_mul(8)
            .saturating_sub(self.pos_bits)
    }

    /// `true` iff [`Self::bits_remaining`] is zero.
    pub fn at_eof(&self) -> bool {
        self.bits_remaining() == 0
    }

    /// Pull one bit. Returns `None` past the end of the buffer.
    pub fn read_bit(&mut self) -> Option<u8> {
        let byte_idx = self.pos_bits / 8;
        let bit_idx = self.pos_bits % 8;
        let b = *self.buf.get(byte_idx)?;
        self.pos_bits += 1;
        Some((b >> (7 - bit_idx)) & 1)
    }

    /// Pull up to 24 bits MSB-first, packed into a `u32`.
    ///
    /// Returns `None` if `nbits` is greater than 24 or if there are
    /// not enough bits in the buffer to cover the request. The 24-bit
    /// limit fits the longest Indeo 2 codeword (13 bits) with plenty
    /// of headroom for table-driven 14-bit lookahead reads.
    pub fn peek_bits(&self, nbits: u8) -> Option<u32> {
        if nbits == 0 {
            return Some(0);
        }
        if nbits > 24 {
            return None;
        }
        if self.bits_remaining() < nbits as usize {
            return None;
        }
        let byte_idx_start = self.pos_bits / 8;
        let bit_idx = self.pos_bits % 8;
        // Pull up to 4 bytes into a 32-bit window, MSB-aligned.
        let mut window: u64 = 0;
        let mut have = 0u8;
        // Load enough bytes to cover bit_idx + nbits.
        let need_bits = bit_idx + nbits as usize;
        let need_bytes = need_bits.div_ceil(8);
        for offset in 0..need_bytes {
            window =
                (window << 8) | self.buf.get(byte_idx_start + offset).copied().unwrap_or(0) as u64;
            have = have.saturating_add(8);
        }
        // Mask off the bit_idx bits we don't want at the top.
        let total_bits_in_window = have as u32;
        let shift = total_bits_in_window
            .checked_sub(bit_idx as u32)
            .and_then(|v| v.checked_sub(nbits as u32))?;
        let value = (window >> shift) & ((1u64 << nbits) - 1);
        Some(value as u32)
    }

    /// Read up to 24 bits MSB-first and advance the cursor.
    pub fn read_bits(&mut self, nbits: u8) -> Option<u32> {
        let v = self.peek_bits(nbits)?;
        self.pos_bits += nbits as usize;
        Some(v)
    }

    /// Drop `nbits` bits without returning them. Returns `false` on
    /// underflow.
    pub fn skip_bits(&mut self, nbits: usize) -> bool {
        if self.bits_remaining() < nbits {
            return false;
        }
        self.pos_bits += nbits;
        true
    }
}

/// Saturating clamp to `0..=255`, used everywhere Indeo applies a
/// signed delta to a previous-pixel reference.
#[inline]
pub fn clip_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bits_msb_first() {
        // 0b10110010 0b11001010 = 0xB2 0xCA
        let buf = [0xB2, 0xCA];
        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(3), Some(0b101)); // top 3 of 0xB2
        assert_eq!(br.read_bits(5), Some(0b10010)); // bottom 5 of 0xB2
        assert_eq!(br.read_bits(4), Some(0b1100)); // top 4 of 0xCA
        assert_eq!(br.read_bits(4), Some(0b1010));
        assert_eq!(br.bits_remaining(), 0);
        assert!(br.at_eof());
    }

    #[test]
    fn peek_does_not_advance() {
        let buf = [0xFF, 0x00];
        let mut br = BitReader::new(&buf);
        assert_eq!(br.peek_bits(8), Some(0xFF));
        assert_eq!(br.peek_bits(8), Some(0xFF));
        assert_eq!(br.read_bits(8), Some(0xFF));
        assert_eq!(br.peek_bits(8), Some(0x00));
    }

    #[test]
    fn read_one_bit_at_a_time() {
        let buf = [0b1010_0110];
        let mut br = BitReader::new(&buf);
        let mut got = 0u8;
        for _ in 0..8 {
            got = (got << 1) | br.read_bit().unwrap();
        }
        assert_eq!(got, 0b1010_0110);
    }

    #[test]
    fn underflow_returns_none() {
        let buf = [0x00];
        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(8), Some(0));
        assert_eq!(br.read_bits(1), None);
        assert_eq!(br.peek_bits(1), None);
    }

    #[test]
    fn clip_u8_clamps_extremes() {
        assert_eq!(clip_u8(-1), 0);
        assert_eq!(clip_u8(0), 0);
        assert_eq!(clip_u8(127), 127);
        assert_eq!(clip_u8(255), 255);
        assert_eq!(clip_u8(256), 255);
        assert_eq!(clip_u8(-1000), 0);
        assert_eq!(clip_u8(1000), 255);
    }
}
