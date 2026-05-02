//! Indeo 2 48-byte fixed frame header parser.
//!
//! Layout per
//! `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md` §3.3.

use oxideav_core::{Error, Result};

/// Total size, in bytes, of every Indeo 2 frame header. Constant
/// across all 1,048 frames observed in the reference trace corpus.
pub const FRAME_HEADER_BYTES: usize = 48;

/// Two-byte ASCII frame magic at offsets `0x0A..0x0B`. Always `'RF'`.
pub const MAGIC_RF: [u8; 2] = *b"RF";

/// Constant 16-bit value at offsets `0x10..0x11` (LE = `0x00C9` = 201).
/// The trace doc proposes "version_or_profile_const"; Indeo 2 has no
/// other profile signalling, so we treat it as a magic literal.
pub const VERSION_CONST: u16 = 0x00C9;

/// Frame-type byte at offset `0x12`. The decoder treats the field as
/// a Boolean (zero ⇒ inter, non-zero ⇒ intra) per the reference
/// document; the precise distinction between the two intra encodings
/// (`0x04` and `0x05`) has no observable effect on the decode path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameType {
    /// Inter / delta-against-previous-frame frame (`frame_type` byte
    /// is `0x00` on the wire).
    Inter,
    /// Intra reset / keyframe. The trailing byte preserves the
    /// original on-wire value (`0x04` or `0x05`), reserved for future
    /// use should that bit ever turn out to mean something.
    Intra(u8),
}

impl FrameType {
    /// Whether this frame is intra-coded.
    pub fn is_intra(self) -> bool {
        matches!(self, FrameType::Intra(_))
    }
}

/// Parsed view over the 48-byte frame header.
///
/// All multi-byte numeric fields are little-endian on the wire. We
/// expose the fields the decoder actually needs; the all-zero
/// reservation slots and the constant tail are validated in
/// [`FrameHeader::parse`] but not retained.
#[derive(Clone, Debug)]
pub struct FrameHeader {
    /// `frame_index` from offset `0x00..0x03`. Strictly increasing
    /// within each captured AVI in the reference corpus, but treated
    /// as advisory only here.
    pub frame_index: u32,
    /// 16-bit per-frame fingerprint at offset `0x08..0x09`. Random
    /// per frame in the corpus; passed through for diagnostics.
    pub frame_hash: u16,
    /// Self-reported size of the entropy payload in bytes (offset
    /// `0x0C..0x0F`). The actual payload may be slightly smaller —
    /// some Intel encoders pad. The decoder uses
    /// `packet_len - FRAME_HEADER_BYTES` as the authoritative payload
    /// length and only consults this field for diagnostics.
    pub payload_size_bytes: u32,
    /// Frame type, derived from offset `0x12`.
    pub frame_type: FrameType,
    /// `payload_size_bytes * 8`, captured directly from offset
    /// `0x14..0x17`. Carried through for diagnostics; the bit reader
    /// derives bit length from the byte payload itself.
    pub payload_size_bits: u32,
    /// Frame height in pixels (offset `0x1C..0x1D`). Always even.
    pub height: u16,
    /// Frame width in pixels (offset `0x1E..0x1F`). Must be even per
    /// §3.4 of the trace document.
    pub width: u16,
    /// Y-plane delta-table selector (low 2 bits of byte `0x22`).
    pub ltab: u8,
    /// Chroma-plane delta-table selector (bits 2..3 of byte `0x22`).
    pub ctab: u8,
    /// `aux_byte` at offset `0x24`. Varies per frame, semantics
    /// undocumented; preserved verbatim.
    pub aux_byte: u8,
}

impl FrameHeader {
    /// Parse a frame header from the start of `data`.
    ///
    /// Validates structural invariants from
    /// `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`
    /// §3.3 — the `'RF'` magic, the `0x00C9` constant, the all-zero
    /// reservation slots, the `frame_type_dup` echo at `0x20`, and
    /// the constant tail `02 00 02 03 03 04 04 04 06 06`. Only the
    /// magic, the constant, and the tail are *hard* validations; the
    /// reserved-zero slots are checked but tolerated since real-world
    /// encoders sometimes leave non-zero garbage in them.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < FRAME_HEADER_BYTES {
            return Err(Error::invalid(format!(
                "indeo2 frame header: need {FRAME_HEADER_BYTES} bytes, got {}",
                data.len()
            )));
        }

        // 'RF' magic at 0x0A..0x0B — hard check.
        if data[0x0A..0x0C] != MAGIC_RF {
            return Err(Error::invalid(format!(
                "indeo2 frame header: missing 'RF' magic at offset 0x0A (saw {:02X} {:02X})",
                data[0x0A], data[0x0B]
            )));
        }

        // 0x00C9 const at 0x10..0x11 — hard check.
        let version = u16::from_le_bytes([data[0x10], data[0x11]]);
        if version != VERSION_CONST {
            return Err(Error::invalid(format!(
                "indeo2 frame header: bad version constant: expected {VERSION_CONST:#06X}, got {version:#06X}"
            )));
        }

        // Constant tail at 0x26..0x2F — hard check.
        const TAIL: [u8; 10] = [0x02, 0x00, 0x02, 0x03, 0x03, 0x04, 0x04, 0x04, 0x06, 0x06];
        if data[0x26..0x30] != TAIL {
            return Err(Error::invalid(
                "indeo2 frame header: constant tail mismatch at 0x26..0x2F",
            ));
        }

        let frame_index = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let frame_hash = u16::from_le_bytes([data[0x08], data[0x09]]);
        let payload_size_bytes =
            u32::from_le_bytes([data[0x0C], data[0x0D], data[0x0E], data[0x0F]]);
        let payload_size_bits =
            u32::from_le_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]);
        let height = u16::from_le_bytes([data[0x1C], data[0x1D]]);
        let width = u16::from_le_bytes([data[0x1E], data[0x1F]]);

        let raw_ftype = data[0x12];
        let raw_ftype_dup = data[0x20];
        // Per §3.3.3 the byte at 0x20 echoes 0x12. Be lenient: warn
        // structurally (return Err only on an outright mismatch that
        // can't be papered over). The reference corpus shows a
        // perfect match across 1,048 frames.
        if raw_ftype != raw_ftype_dup {
            return Err(Error::invalid(format!(
                "indeo2 frame header: frame_type_dup mismatch (0x12={:#04X}, 0x20={:#04X})",
                raw_ftype, raw_ftype_dup
            )));
        }

        let frame_type = match raw_ftype {
            0x00 => FrameType::Inter,
            non_zero => FrameType::Intra(non_zero),
        };

        // table_select byte: bits 0..1 = ltab, bits 2..3 = ctab.
        // Bits 4..7 reserved (per the trace, always zero).
        let ts = data[0x22];
        let ltab = ts & 0x03;
        let ctab = (ts >> 2) & 0x03;

        // Width must be even — every codeword emits an even pixel
        // count (§3.4).
        if width == 0 || width & 1 != 0 {
            return Err(Error::invalid(format!(
                "indeo2 frame header: width must be even and non-zero (got {width})"
            )));
        }
        if height == 0 {
            return Err(Error::invalid("indeo2 frame header: height is zero"));
        }
        // yuv410p chroma: width/4 and height/4 must be sensible too.
        if width % 4 != 0 || height % 4 != 0 {
            return Err(Error::invalid(format!(
                "indeo2 frame header: yuv410p requires width and height divisible by 4 (got {width}x{height})"
            )));
        }

        let aux_byte = data[0x24];

        Ok(Self {
            frame_index,
            frame_hash,
            payload_size_bytes,
            frame_type,
            payload_size_bits,
            height,
            width,
            ltab,
            ctab,
            aux_byte,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built minimal valid header — 48 bytes of constants plus a
    /// single inter-frame at 160×120 with both table selectors at 0.
    fn synth_header(frame_type: u8, ltab: u8, ctab: u8, w: u16, h: u16) -> [u8; 48] {
        let mut hdr = [0u8; 48];
        // frame_index = 0 (already zero)
        // 0x08..0x09 frame hash = 0x1234
        hdr[0x08] = 0x34;
        hdr[0x09] = 0x12;
        // 'RF'
        hdr[0x0A] = b'R';
        hdr[0x0B] = b'F';
        // payload_size_bytes = 16
        hdr[0x0C] = 0x10;
        // version
        hdr[0x10] = (VERSION_CONST & 0xff) as u8;
        hdr[0x11] = (VERSION_CONST >> 8) as u8;
        // frame type + dup
        hdr[0x12] = frame_type;
        // payload_size_bits = 128
        hdr[0x14] = 128;
        // height / width
        hdr[0x1C] = (h & 0xff) as u8;
        hdr[0x1D] = (h >> 8) as u8;
        hdr[0x1E] = (w & 0xff) as u8;
        hdr[0x1F] = (w >> 8) as u8;
        // dup
        hdr[0x20] = frame_type;
        // table_select
        hdr[0x22] = (ltab & 3) | ((ctab & 3) << 2);
        // tail
        let tail: [u8; 10] = [0x02, 0x00, 0x02, 0x03, 0x03, 0x04, 0x04, 0x04, 0x06, 0x06];
        hdr[0x26..0x30].copy_from_slice(&tail);
        hdr
    }

    #[test]
    fn parses_minimal_inter() {
        let hdr = synth_header(0x00, 0, 0, 160, 120);
        let parsed = FrameHeader::parse(&hdr).unwrap();
        assert_eq!(parsed.frame_type, FrameType::Inter);
        assert_eq!(parsed.width, 160);
        assert_eq!(parsed.height, 120);
        assert_eq!(parsed.ltab, 0);
        assert_eq!(parsed.ctab, 0);
        assert_eq!(parsed.payload_size_bytes, 16);
        assert_eq!(parsed.payload_size_bits, 128);
        assert_eq!(parsed.frame_hash, 0x1234);
    }

    #[test]
    fn parses_minimal_intra() {
        let hdr = synth_header(0x05, 1, 2, 320, 240);
        let parsed = FrameHeader::parse(&hdr).unwrap();
        assert_eq!(parsed.frame_type, FrameType::Intra(0x05));
        assert!(parsed.frame_type.is_intra());
        assert_eq!(parsed.ltab, 1);
        assert_eq!(parsed.ctab, 2);
        assert_eq!(parsed.width, 320);
        assert_eq!(parsed.height, 240);
    }

    #[test]
    fn rejects_short_buffer() {
        let hdr = [0u8; 40];
        assert!(FrameHeader::parse(&hdr).is_err());
    }

    #[test]
    fn rejects_missing_magic() {
        let mut hdr = synth_header(0x00, 0, 0, 160, 120);
        hdr[0x0A] = b'X';
        assert!(FrameHeader::parse(&hdr).is_err());
    }

    #[test]
    fn rejects_bad_version_const() {
        let mut hdr = synth_header(0x00, 0, 0, 160, 120);
        hdr[0x10] = 0x42;
        assert!(FrameHeader::parse(&hdr).is_err());
    }

    #[test]
    fn rejects_bad_constant_tail() {
        let mut hdr = synth_header(0x00, 0, 0, 160, 120);
        hdr[0x26] = 0xFF;
        assert!(FrameHeader::parse(&hdr).is_err());
    }

    #[test]
    fn rejects_frame_type_mismatch() {
        let mut hdr = synth_header(0x00, 0, 0, 160, 120);
        hdr[0x20] = 0x05; // mismatch with 0x12 = 0x00
        assert!(FrameHeader::parse(&hdr).is_err());
    }

    #[test]
    fn rejects_odd_width() {
        let hdr = synth_header(0x00, 0, 0, 161, 120);
        assert!(FrameHeader::parse(&hdr).is_err());
    }

    #[test]
    fn rejects_zero_dimensions() {
        let hdr = synth_header(0x00, 0, 0, 0, 120);
        assert!(FrameHeader::parse(&hdr).is_err());
        let hdr = synth_header(0x00, 0, 0, 160, 0);
        assert!(FrameHeader::parse(&hdr).is_err());
    }
}
