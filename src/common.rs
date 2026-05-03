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
/// frame header. The byte stream is consumed left-to-right; bits within
/// each byte are pulled **LSB-first** (so bit 0 of byte 0 is bit 0 of
/// the bit-stream, bit 1 of byte 0 is bit 1 of the bit-stream, …, bit
/// 7 of byte 0 is bit 7, and bit 0 of byte 1 is bit 8). Multi-bit
/// reads return the bits in the order they were consumed — i.e. for a
/// 3-bit read, the first-consumed bit lands in the MSB of the result.
///
/// This matches FFmpeg's `BitstreamContextLE`: a code that the trace
/// document writes as a binary string MSB-first (e.g. symbol 0x01 ⇒
/// `000`) is read by stepping through the LE bit-stream and packing
/// the consumed bits into a value MSB-first. The 14-bit Huffman
/// lookup window in `super::v2::huffman` indexes its table with the
/// resulting MSB-first integer.
///
/// `pos_bits()` reports the current cursor in bits from the start of
/// the underlying buffer, which is what `[TRACE/code]` lines in the
/// reference document carry.
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
    ///
    /// The bit at byte position `b`, intra-byte position `i` (LSB =
    /// 0) is returned for the `b*8 + i`-th call, matching the LE bit
    /// reader described in the type-level docs.
    pub fn read_bit(&mut self) -> Option<u8> {
        let byte_idx = self.pos_bits / 8;
        let bit_idx = self.pos_bits % 8;
        let b = *self.buf.get(byte_idx)?;
        self.pos_bits += 1;
        Some((b >> bit_idx) & 1)
    }

    /// Pull up to 24 bits, packed MSB-first into the returned `u32`.
    ///
    /// Successive bits in the bit-stream become successive bits of
    /// the result *from MSB to LSB*: the first bit consumed lands at
    /// `1 << (nbits - 1)`, the last bit consumed lands at `1 << 0`.
    /// Within each byte of the underlying buffer, bits are stepped
    /// LSB-first.
    ///
    /// Returns `None` if `nbits` is greater than 24 or if there are
    /// not enough bits in the buffer. The 24-bit limit fits the
    /// longest Indeo 2 codeword (14 bits) with comfortable headroom.
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
        let mut value: u32 = 0;
        for i in 0..nbits as usize {
            let p = self.pos_bits + i;
            let byte_idx = p / 8;
            let bit_idx = p % 8;
            let b = self.buf[byte_idx];
            let bit = (b >> bit_idx) & 1;
            // First bit consumed → MSB of `value`.
            value |= (bit as u32) << (nbits as usize - 1 - i);
        }
        Some(value)
    }

    /// Read up to 24 bits and advance the cursor. Bits are packed
    /// MSB-first into the returned value (see [`Self::peek_bits`]).
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
    fn read_bits_le_byte_msb_pack() {
        // 0xB2 = 0b1011_0010. Bits in the bit-stream order (LSB-first
        // within byte): 0,1,0,0,1,1,0,1.
        // 0xCA = 0b1100_1010. Bits in stream order: 0,1,0,1,0,0,1,1.
        let buf = [0xB2, 0xCA];
        let mut br = BitReader::new(&buf);
        // First 3 bits consumed = (0,1,0). Packed MSB-first: 0b010 = 2.
        assert_eq!(br.read_bits(3), Some(0b010));
        // Next 5 bits = (0,1,1,0,1) MSB-first → 0b01101 = 13.
        assert_eq!(br.read_bits(5), Some(0b01101));
        // Next 4 bits = (0,1,0,1) MSB-first → 0b0101 = 5.
        assert_eq!(br.read_bits(4), Some(0b0101));
        // Final 4 bits = (0,0,1,1) MSB-first → 0b0011 = 3.
        assert_eq!(br.read_bits(4), Some(0b0011));
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
        // 0b1010_0110. LSB-first: 0,1,1,0,0,1,0,1.
        let buf = [0b1010_0110];
        let mut br = BitReader::new(&buf);
        let expected = [0u8, 1, 1, 0, 0, 1, 0, 1];
        for &e in &expected {
            assert_eq!(br.read_bit().unwrap(), e);
        }
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
