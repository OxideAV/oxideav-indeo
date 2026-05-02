//! Indeo 2 packet → video frame decoder.
//!
//! Round 1: parses the 48-byte frame header and validates it against
//! the structural invariants from the reference trace document. The
//! per-pixel pair / run entropy decode is deferred to round 2 — see
//! the crate-level docs for the specifics. The decoder still emits a
//! `VideoFrame` with the correct dimensions / pixel format / plane
//! layout so it can be wired into the pipeline today; pixel values
//! are placeholder mid-grey for the luma plane and neutral chroma for
//! U / V.

use oxideav_core::frame::VideoPlane;
use oxideav_core::{
    CodecId, CodecParameters, Decoder, Error, Frame, Packet, PixelFormat, Result, VideoFrame,
};

use crate::common::BitReader;
use crate::v2::header::{FrameHeader, FRAME_HEADER_BYTES};

/// Intel Indeo 2 single-stream decoder.
///
/// One persistent reference buffer (Y, U, V) is retained across
/// frames so inter packets can be reconstructed against the previous
/// decoded frame as required by §3.7 / §3.9 of the trace doc. Round 1
/// allocates the reference buffer but does not yet produce
/// fully-decoded pixels — see crate docs.
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
        let decoded = decode_packet(&pkt.data, self.prev.as_ref())?;

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
/// Round 1 verifies the frame header, sanity-checks the entropy
/// payload size, and produces a [`DecodedFrame`] of the correct
/// dimensions. Pixel values are placeholders (luma 128, chroma 128)
/// because the static codeword table and the four delta tables are
/// not yet derived — this is the round-2 deliverable. The bit reader
/// is exercised over the entropy payload anyway so the wiring is
/// proven end-to-end against real fixtures.
pub(crate) fn decode_packet(data: &[u8], _prev: Option<&DecodedFrame>) -> Result<DecodedFrame> {
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

    // Exercise the bit reader to confirm wiring — this is a no-op
    // proof-of-life pending the real entropy decoder. Without the
    // codeword tables we can't actually emit pixels here.
    let mut br = BitReader::new(payload);
    // Walk a token's worth of bits; this surfaces any underflow
    // immediately rather than silently returning garbage. Use a small
    // fixed budget so the test stays deterministic.
    let walk_bits = std::cmp::min(payload.len() * 8, 64);
    for _ in 0..walk_bits {
        if br.read_bit().is_none() {
            return Err(Error::invalid(
                "indeo2: bit reader underflow before token budget exhausted",
            ));
        }
    }

    // Round 1: emit a structurally valid placeholder frame at the
    // bitstream's declared dimensions. The next round wires the
    // actual pair / run plane decoder into this slot.
    let frame = DecodedFrame::new(header.width, header.height);
    let _ = (header.frame_type, header.ltab, header.ctab); // FUTURE
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::header::{FRAME_HEADER_BYTES, MAGIC_RF, VERSION_CONST};

    fn synth_packet(frame_type: u8, w: u16, h: u16, payload_bytes: usize) -> Vec<u8> {
        let mut pkt = vec![0u8; FRAME_HEADER_BYTES + payload_bytes];
        // 'RF' magic
        pkt[0x0A..0x0C].copy_from_slice(&MAGIC_RF);
        // version const
        pkt[0x10] = (VERSION_CONST & 0xff) as u8;
        pkt[0x11] = (VERSION_CONST >> 8) as u8;
        // frame type + dup
        pkt[0x12] = frame_type;
        pkt[0x20] = frame_type;
        // payload byte count
        let plb = payload_bytes as u32;
        pkt[0x0C..0x10].copy_from_slice(&plb.to_le_bytes());
        let plbits = plb.saturating_mul(8);
        pkt[0x14..0x18].copy_from_slice(&plbits.to_le_bytes());
        // dims
        pkt[0x1C..0x1E].copy_from_slice(&h.to_le_bytes());
        pkt[0x1E..0x20].copy_from_slice(&w.to_le_bytes());
        // tail
        pkt[0x26..0x30]
            .copy_from_slice(&[0x02, 0x00, 0x02, 0x03, 0x03, 0x04, 0x04, 0x04, 0x06, 0x06]);
        // Some non-zero payload bytes so the bit reader has work.
        for (i, b) in pkt[FRAME_HEADER_BYTES..].iter_mut().enumerate() {
            *b = ((i as u32 * 17) & 0xff) as u8;
        }
        pkt
    }

    #[test]
    fn decodes_synthetic_intra_frame() {
        // 160x120 intra @ 1024 B payload — comfortably above the
        // (160*120)/32 = 600 B minimum.
        let pkt = synth_packet(0x05, 160, 120, 1024);
        let df = decode_packet(&pkt, None).unwrap();
        assert_eq!(df.width, 160);
        assert_eq!(df.height, 120);
        assert_eq!(df.y_plane.len(), 160 * 120);
        assert_eq!(df.u_plane.len(), 40 * 30);
        assert_eq!(df.v_plane.len(), 40 * 30);
    }

    #[test]
    fn decodes_synthetic_inter_frame() {
        let pkt = synth_packet(0x00, 160, 120, 256);
        let df = decode_packet(&pkt, None).unwrap();
        assert_eq!(df.width, 160);
        assert_eq!(df.height, 120);
    }

    #[test]
    fn rejects_short_intra() {
        // 160x120 intra needs at least 600 B; give it 100.
        let pkt = synth_packet(0x05, 160, 120, 100);
        assert!(decode_packet(&pkt, None).is_err());
    }

    #[test]
    fn rejects_packet_with_no_payload() {
        let pkt = synth_packet(0x00, 160, 120, 0);
        assert!(decode_packet(&pkt, None).is_err());
    }

    #[test]
    fn produces_video_frame_with_correct_layout() {
        let pkt = synth_packet(0x05, 160, 120, 1024);
        let df = decode_packet(&pkt, None).unwrap();
        let vf = df.to_video_frame(Some(42));
        assert_eq!(vf.pts, Some(42));
        assert_eq!(vf.planes.len(), 3);
        // Yuv420P chroma is (w/2) x (h/2) — 80 x 60 here.
        assert_eq!(vf.planes[0].data.len(), 160 * 120);
        assert_eq!(vf.planes[1].data.len(), 80 * 60);
        assert_eq!(vf.planes[2].data.len(), 80 * 60);
        assert_eq!(vf.planes[0].stride, 160);
        assert_eq!(vf.planes[1].stride, 80);
    }

    #[test]
    fn full_decoder_round_trip_via_trait() {
        use oxideav_core::TimeBase;
        let pkt_data = synth_packet(0x04, 160, 120, 1024);
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
            assert_eq!(vf.planes[0].data.len(), 160 * 120);
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
