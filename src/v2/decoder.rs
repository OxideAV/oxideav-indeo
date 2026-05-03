//! Indeo 2 packet → video frame decoder.
//!
//! Round 2: parses the 48-byte frame header and runs the full pair /
//! run entropy decode against the 143-entry canonical Huffman codebook
//! and the four 256-byte delta tables (see `super::tables` and
//! `super::huffman`). Output is yuv410p internally, expanded to
//! `Yuv420P` on emission. Inter packets are reconstructed against the
//! previous decoded frame as required by §3.7 / §3.9 of the trace
//! document.

use oxideav_core::frame::VideoPlane;
use oxideav_core::{
    CodecId, CodecParameters, Decoder, Error, Frame, Packet, PixelFormat, Result, VideoFrame,
};

use crate::common::BitReader;
use crate::v2::header::{FrameHeader, FrameType, FRAME_HEADER_BYTES};
use crate::v2::huffman::HuffTable;
use crate::v2::plane::decode_plane;
use crate::v2::tables::DELTA_TABLES;

/// Intel Indeo 2 single-stream decoder.
///
/// One persistent reference buffer (Y, U, V) is retained across
/// frames so inter packets can be reconstructed against the previous
/// decoded frame as required by §3.7 / §3.9 of the trace doc. The
/// 14-bit Huffman lookup table is built once and shared across all
/// frames.
pub struct Indeo2Decoder {
    codec_id: CodecId,
    pending: Option<Packet>,
    eof: bool,
    /// Last decoded frame's planes, reused as the previous-frame
    /// reference for inter packets. Kept as raw `yuv410p` (Y + U + V
    /// at 4:1:1 in *both* dimensions) before being expanded to
    /// `Yuv420P` for emission.
    prev: Option<DecodedFrame>,
    /// (width, height) of the last decoded frame. Mismatching
    /// dimensions on the next packet invalidate `prev`.
    dims: Option<(u16, u16)>,
    /// 14-bit Huffman lookup, shared across frames.
    huff: HuffTable,
}

impl std::fmt::Debug for Indeo2Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Indeo2Decoder")
            .field("codec_id", &self.codec_id)
            .field("has_pending", &self.pending.is_some())
            .field("eof", &self.eof)
            .field("dims", &self.dims)
            .finish()
    }
}

impl Indeo2Decoder {
    pub fn new(codec_id: CodecId) -> Self {
        Self {
            codec_id,
            pending: None,
            eof: false,
            prev: None,
            dims: None,
            huff: HuffTable::build(),
        }
    }
}

/// Decoder factory — constructs a fresh [`Indeo2Decoder`].
///
/// Wired in from the workspace registry via `lib.rs::register_indeo2`.
pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    Ok(Box::new(Indeo2Decoder::new(params.codec_id.clone())))
}

impl Decoder for Indeo2Decoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::other(
                "indeo2: receive_frame must be called before sending another packet",
            ));
        }
        self.pending = Some(packet.clone());
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        let Some(pkt) = self.pending.take() else {
            return if self.eof {
                Err(Error::Eof)
            } else {
                Err(Error::NeedMore)
            };
        };
        let decoded = decode_packet(&self.huff, &pkt.data, self.prev.as_ref())?;

        // Dimension change invalidates the reference buffer for any
        // subsequent inter packet — but the trace doc shows every
        // first frame is intra (`frame_type` 0x04 / 0x05) so this is
        // mostly defensive.
        if self.dims != Some((decoded.width, decoded.height)) {
            self.dims = Some((decoded.width, decoded.height));
        }
        let frame = decoded.to_video_frame(pkt.pts);
        self.prev = Some(decoded);
        Ok(Frame::Video(frame))
    }

    fn flush(&mut self) -> Result<()> {
        self.eof = true;
        Ok(())
    }
}

/// Output pixel format. Indeo 2's native chroma layout is `yuv410p`
/// (one chroma sample per 4×4 luma block) but `oxideav-core` does not
/// yet expose a `yuv410p` `PixelFormat` variant. We expand to the
/// closest match — `Yuv420P` — by 2×2-replicating each chroma sample
/// into its 2×2 chroma block. Switching to a future `Yuv410P` variant
/// is a one-line change.
pub const OUTPUT_PIX_FMT: PixelFormat = PixelFormat::Yuv420P;

/// Internal one-frame raster. Stores the native `yuv410p` layout
/// (chroma is `(width/4) × (height/4)`); conversion to the emitted
/// `Yuv420P` happens on the way out via [`DecodedFrame::to_video_frame`].
#[derive(Clone, Debug)]
pub(crate) struct DecodedFrame {
    pub width: u16,
    pub height: u16,
    /// `width * height` Y samples.
    pub y_plane: Vec<u8>,
    /// `(width/4) * (height/4)` U samples.
    pub u_plane: Vec<u8>,
    /// `(width/4) * (height/4)` V samples.
    pub v_plane: Vec<u8>,
}

impl DecodedFrame {
    fn new(width: u16, height: u16) -> Self {
        let w = width as usize;
        let h = height as usize;
        let chroma_w = w / 4;
        let chroma_h = h / 4;
        Self {
            width,
            height,
            y_plane: vec![128; w * h],
            u_plane: vec![128; chroma_w * chroma_h],
            v_plane: vec![128; chroma_w * chroma_h],
        }
    }

    /// Plane sizes for a (width, height) frame in `yuv410p` layout.
    fn plane_dims(width: u16, height: u16) -> (usize, usize, usize, usize) {
        let w = width as usize;
        let h = height as usize;
        (w, h, w / 4, h / 4)
    }

    /// Expand the native yuv410p planes into a Yuv420P `VideoFrame`.
    ///
    /// Each yuv410p chroma sample covers a 4×4 luma block; the
    /// matching yuv420p chroma sample covers a 2×2 luma block, so
    /// every yuv410p chroma sample maps to a 2×2 patch of yuv420p
    /// chroma. We perform the simplest replication (no filter); a
    /// real `Yuv410P` `PixelFormat` would let us emit this without
    /// the up-sample.
    fn to_video_frame(&self, pts: Option<i64>) -> VideoFrame {
        let w = self.width as usize;
        let h = self.height as usize;
        let chroma_w_410 = w / 4;
        let chroma_w_420 = w / 2;
        let chroma_h_420 = h / 2;

        let mut u_out = vec![128u8; chroma_w_420 * chroma_h_420];
        let mut v_out = vec![128u8; chroma_w_420 * chroma_h_420];
        for y_410 in 0..(h / 4) {
            for x_410 in 0..chroma_w_410 {
                let u = self.u_plane[y_410 * chroma_w_410 + x_410];
                let v = self.v_plane[y_410 * chroma_w_410 + x_410];
                // Each yuv410 chroma sample owns a 2x2 yuv420 block.
                for dy in 0..2 {
                    for dx in 0..2 {
                        let y_420 = y_410 * 2 + dy;
                        let x_420 = x_410 * 2 + dx;
                        let off = y_420 * chroma_w_420 + x_420;
                        u_out[off] = u;
                        v_out[off] = v;
                    }
                }
            }
        }

        VideoFrame {
            pts,
            planes: vec![
                VideoPlane {
                    stride: w,
                    data: self.y_plane.clone(),
                },
                VideoPlane {
                    stride: chroma_w_420,
                    data: u_out,
                },
                VideoPlane {
                    stride: chroma_w_420,
                    data: v_out,
                },
            ],
        }
    }
}

/// Decode one whole-frame packet.
///
/// Verifies the frame header, then runs the pair / run plane decoder
/// for the on-wire plane order Y, V, U. Inter frames are
/// reconstructed against `prev` (the previous decoded frame); intra
/// frames discard `prev`.
pub(crate) fn decode_packet(
    huff: &HuffTable,
    data: &[u8],
    prev: Option<&DecodedFrame>,
) -> Result<DecodedFrame> {
    let header = FrameHeader::parse(data)?;
    if data.len() <= FRAME_HEADER_BYTES {
        return Err(Error::invalid(format!(
            "indeo2: packet of {} bytes has no entropy payload after the {}-byte header",
            data.len(),
            FRAME_HEADER_BYTES
        )));
    }

    // Authoritative payload length is everything after the header —
    // the trace doc notes encoders sometimes pad, so trust the packet
    // boundary, not the in-header byte count.
    let payload = &data[FRAME_HEADER_BYTES..];

    // Lower-bound check from §3.8: an intra payload must be at least
    // width * height / 32 bytes — the absolute minimum if every
    // codeword were the maximum-length 32-pixel run code (with run
    // codes generally 3..5 bits). Skip the check for inter frames
    // since the trace doc says the upstream encoder doesn't emit one.
    if header.frame_type.is_intra() {
        let min_payload = (header.width as usize * header.height as usize) / 32;
        if payload.len() < min_payload {
            return Err(Error::invalid(format!(
                "indeo2 intra: payload {} B below minimum {} B for {}x{}",
                payload.len(),
                min_payload,
                header.width,
                header.height
            )));
        }
    }

    let intra = matches!(header.frame_type, FrameType::Intra(_));
    let (yw, yh, cw, ch) = DecodedFrame::plane_dims(header.width, header.height);

    // Pick the active delta tables.
    if header.ltab > 3 || header.ctab > 3 {
        return Err(Error::invalid(format!(
            "indeo2: ltab={} ctab={} out of range",
            header.ltab, header.ctab
        )));
    }
    let l_table = &DELTA_TABLES[header.ltab as usize];
    let c_table = &DELTA_TABLES[header.ctab as usize];

    // For inter, seed plane buffers from the previous frame; for
    // intra, discard. If prev's dimensions don't match the header
    // (e.g. dimension change), fall back to neutral and treat as
    // intra-style decode is not safe — we error out instead.
    let mut frame = if intra {
        DecodedFrame::new(header.width, header.height)
    } else {
        match prev {
            Some(p) if p.width == header.width && p.height == header.height => p.clone(),
            Some(_) => {
                return Err(Error::invalid(
                    "indeo2 inter: previous-frame dimensions disagree with current header",
                ));
            }
            None => {
                return Err(Error::invalid(
                    "indeo2 inter: no previous frame available — first frame must be intra",
                ));
            }
        }
    };

    let mut br = BitReader::new(payload);
    // Wire-order: Y, V, U (note chroma swap relative to natural YUV).
    decode_plane(huff, l_table, &mut br, &mut frame.y_plane, yw, yh, intra)?;
    decode_plane(huff, c_table, &mut br, &mut frame.v_plane, cw, ch, intra)?;
    decode_plane(huff, c_table, &mut br, &mut frame.u_plane, cw, ch, intra)?;

    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::header::{FRAME_HEADER_BYTES, MAGIC_RF, VERSION_CONST};

    /// Build a header-only packet (with the supplied payload appended).
    fn synth_packet_with_payload(frame_type: u8, w: u16, h: u16, payload: &[u8]) -> Vec<u8> {
        let mut pkt = vec![0u8; FRAME_HEADER_BYTES + payload.len()];
        pkt[0x0A..0x0C].copy_from_slice(&MAGIC_RF);
        pkt[0x10] = (VERSION_CONST & 0xff) as u8;
        pkt[0x11] = (VERSION_CONST >> 8) as u8;
        pkt[0x12] = frame_type;
        pkt[0x20] = frame_type;
        let plb = payload.len() as u32;
        pkt[0x0C..0x10].copy_from_slice(&plb.to_le_bytes());
        let plbits = plb.saturating_mul(8);
        pkt[0x14..0x18].copy_from_slice(&plbits.to_le_bytes());
        pkt[0x1C..0x1E].copy_from_slice(&h.to_le_bytes());
        pkt[0x1E..0x20].copy_from_slice(&w.to_le_bytes());
        pkt[0x26..0x30]
            .copy_from_slice(&[0x02, 0x00, 0x02, 0x03, 0x03, 0x04, 0x04, 0x04, 0x06, 0x06]);
        pkt[FRAME_HEADER_BYTES..].copy_from_slice(payload);
        pkt
    }

    /// Pack a sequence of bit values into bytes, LSB-first within
    /// each byte (matching `crate::common::BitReader`'s wire order).
    fn pack_bits(bits: &[u8]) -> Vec<u8> {
        let mut out = vec![];
        let mut acc: u8 = 0;
        let mut nb = 0u8;
        for &b in bits {
            acc |= (b & 1) << nb;
            nb += 1;
            if nb == 8 {
                out.push(acc);
                acc = 0;
                nb = 0;
            }
        }
        if nb > 0 {
            out.push(acc);
        }
        out
    }

    /// Build a payload that's `n` repetitions of pair-symbol-1
    /// codeword (binary `000`, three bits).
    fn pair1_payload(n: usize) -> Vec<u8> {
        let bits: Vec<u8> = (0..n).flat_map(|_| [0, 0, 0]).collect();
        pack_bits(&bits)
    }

    /// Build a payload that's `n` repetitions of run-2 codeword
    /// (binary `010`, three bits — symbol 0x80, run 2 px).
    fn run2_payload(n: usize) -> Vec<u8> {
        let bits: Vec<u8> = (0..n).flat_map(|_| [0, 1, 0]).collect();
        pack_bits(&bits)
    }

    #[test]
    fn decodes_synthetic_intra_frame_8x8() {
        // 8×8 intra. Y plane = 64 px = 32 pairs; chroma = 2×2 = 4 px
        // = 2 pairs each plane. Total 36 pair codewords.
        let payload = pair1_payload(32 + 2 + 2);
        let pkt = synth_packet_with_payload(0x05, 8, 8, &payload);
        let huff = HuffTable::build();
        let df = decode_packet(&huff, &pkt, None).unwrap();
        assert_eq!(df.width, 8);
        assert_eq!(df.height, 8);
        assert_eq!(df.y_plane.len(), 64);
        assert_eq!(df.u_plane.len(), 4);
        assert_eq!(df.v_plane.len(), 4);
        // Row 0 of Y: every pair-1 emits (0x84, 0x84) into the
        // absolute palette.
        assert_eq!(&df.y_plane[..8], &[0x84; 8]);
        // Row 1 of Y: pair-1 again, treated as +4 delta vs row 0.
        assert_eq!(&df.y_plane[8..16], &[0x88; 8]);
    }

    #[test]
    fn decodes_synthetic_inter_frame_8x8() {
        // Build a prev frame at 8×8.
        let prev = DecodedFrame::new(8, 8);
        // Inter: every codeword uses 3/4 scaled delta vs prev. Pair-1
        // = +4 delta -> +3 scaled. Y plane = 32 pair codewords;
        // chroma = 2 each.
        let payload = pair1_payload(32 + 2 + 2);
        let pkt = synth_packet_with_payload(0x00, 8, 8, &payload);
        let huff = HuffTable::build();
        let df = decode_packet(&huff, &pkt, Some(&prev)).unwrap();
        // Prev was filled with 128; +3 yields 131 everywhere.
        assert!(df.y_plane.iter().all(|&v| v == 131));
    }

    #[test]
    fn inter_run_skip_preserves_prev_pixels() {
        let mut prev = DecodedFrame::new(8, 8);
        // Mark prev with a pattern.
        for (i, p) in prev.y_plane.iter_mut().enumerate() {
            *p = (i as u8).wrapping_mul(7).wrapping_add(50);
        }
        for p in prev.u_plane.iter_mut() {
            *p = 100;
        }
        for p in prev.v_plane.iter_mut() {
            *p = 200;
        }
        // 8×8 = 64 px, run-2 codeword pixels-per-symbol = 2, need 32
        // run codewords. Chroma is 2×2 = 4 px = 2 run codewords each.
        let payload = run2_payload(32 + 2 + 2);
        let pkt = synth_packet_with_payload(0x00, 8, 8, &payload);
        let huff = HuffTable::build();
        let df = decode_packet(&huff, &pkt, Some(&prev)).unwrap();
        assert_eq!(df.y_plane, prev.y_plane);
        assert_eq!(df.u_plane, prev.u_plane);
        assert_eq!(df.v_plane, prev.v_plane);
    }

    #[test]
    fn rejects_short_intra() {
        // 160x120 intra needs at least 600 B; give it 100.
        let payload = vec![0u8; 100];
        let pkt = synth_packet_with_payload(0x05, 160, 120, &payload);
        let huff = HuffTable::build();
        assert!(decode_packet(&huff, &pkt, None).is_err());
    }

    #[test]
    fn rejects_packet_with_no_payload() {
        let pkt = synth_packet_with_payload(0x00, 160, 120, &[]);
        let huff = HuffTable::build();
        assert!(decode_packet(&huff, &pkt, None).is_err());
    }

    #[test]
    fn rejects_inter_without_prev() {
        let payload = pair1_payload(32 + 2 + 2);
        let pkt = synth_packet_with_payload(0x00, 8, 8, &payload);
        let huff = HuffTable::build();
        assert!(decode_packet(&huff, &pkt, None).is_err());
    }

    #[test]
    fn produces_video_frame_with_correct_layout_8x8() {
        let payload = pair1_payload(32 + 2 + 2);
        let pkt = synth_packet_with_payload(0x05, 8, 8, &payload);
        let huff = HuffTable::build();
        let df = decode_packet(&huff, &pkt, None).unwrap();
        let vf = df.to_video_frame(Some(42));
        assert_eq!(vf.pts, Some(42));
        assert_eq!(vf.planes.len(), 3);
        // Yuv420P chroma is (w/2) x (h/2) — 4 x 4 here.
        assert_eq!(vf.planes[0].data.len(), 8 * 8);
        assert_eq!(vf.planes[1].data.len(), 4 * 4);
        assert_eq!(vf.planes[2].data.len(), 4 * 4);
        assert_eq!(vf.planes[0].stride, 8);
        assert_eq!(vf.planes[1].stride, 4);
    }

    #[test]
    fn full_decoder_round_trip_via_trait() {
        use oxideav_core::TimeBase;
        let payload = pair1_payload(32 + 2 + 2);
        let pkt_data = synth_packet_with_payload(0x04, 8, 8, &payload);
        let mut dec = Indeo2Decoder::new(CodecId::new("indeo2"));
        // Trying to receive a frame before sending must yield NeedMore.
        match dec.receive_frame() {
            Err(Error::NeedMore) => {}
            other => panic!("expected NeedMore, got {:?}", other.map(|_| "frame")),
        }
        let pkt = Packet::new(0, TimeBase::new(1, 15), pkt_data);
        dec.send_packet(&pkt).unwrap();
        let frame = dec.receive_frame().unwrap();
        if let Frame::Video(vf) = frame {
            assert_eq!(vf.planes.len(), 3);
            assert_eq!(vf.planes[0].data.len(), 8 * 8);
        } else {
            panic!("expected Frame::Video");
        }
        // After flush, with no pending packet, we should reach Eof.
        dec.flush().unwrap();
        match dec.receive_frame() {
            Err(Error::Eof) => {}
            other => panic!("expected Eof, got {:?}", other.map(|_| "frame")),
        }
    }
}
