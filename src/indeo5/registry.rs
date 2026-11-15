//! Indeo 5 (`IV50`) codec-registry integration: the [`oxideav_core`]
//! [`Decoder`] trait wrapper around the in-crate [`Iv50Decoder`], plus
//! the FourCC routing surface a generic container (`oxideav-avi`,
//! `oxideav-mov`) needs to resolve an `IV50` video track to this crate.
//!
//! * [`codec_id_for_fourcc`] maps an on-wire FourCC (`IV50`,
//!   case-insensitive) to the [`CodecId`] this crate registers.
//! * [`Indeo5RegistryDecoder`] implements [`Decoder`]: it owns an
//!   [`Iv50Decoder`], feeds each [`Packet`]'s bytes to
//!   [`Iv50Decoder::decode`] and turns the decoded planes into an
//!   [`oxideav_core::VideoFrame`]. A NULL frame repeats the previous
//!   picture.
//! * [`make_decoder`] is the [`oxideav_core::registry::codec::DecoderFactory`]
//!   the registry calls; [`register_codecs`] / [`register`] install the
//!   codec (id + caps + factory + probe + FourCC tag) into a
//!   [`CodecRegistry`] / [`RuntimeContext`].
//!
//! ## Output pixel format and stream parameters
//!
//! The GOP header of every intra frame carries the picture size and the
//! chroma layout (gop_flags bit 1), so the decoder describes its own
//! output; the container's width and height are advisory.
//!
//! * **Quarter-size chroma** (4:1:0, bit 1 clear; every stream staged so
//!   far): the framework has no 4:1:0 pixel format, so each chroma
//!   sample is repeated over the 4 by 4 luma area it covers and the
//!   frame is emitted as [`PixelFormat::Yuv444P`], three planes at the
//!   luma size.
//! * **Half-size chroma** (4:2:0, bit 1 set): the planes are emitted as
//!   decoded, [`PixelFormat::Yuv420P`], chroma `ceil(w / 2)` by
//!   `ceil(h / 2)`.
//!
//! Planes are in Y, U, V order with the stride equal to the plane
//! width. Size and pixel format are stream metadata, never per frame:
//! [`Indeo5RegistryDecoder`] reports them through
//! [`Decoder::output_params`] once the first frame has been returned,
//! and when an intra frame changes them mid-stream it moves
//! [`Decoder::output_params_generation`] and returns one
//! [`Frame::StreamParams`] event immediately before the first frame of
//! the new shape.

use oxideav_core::{
    CodecCapabilities, CodecId, CodecInfo, CodecParameters, CodecRegistry, CodecTag, Decoder,
    Error, Frame, Packet, PixelFormat, ProbeContext, Result, RuntimeContext, VideoFrame,
    VideoPlane,
};

use super::decoder::{Iv50Decoder, Iv50Error, Iv50Plane, looks_like_iv50};

/// The public codec id this crate registers for Indeo 5 (`"indeo5"`).
pub const CODEC_ID_STR: &str = "indeo5";

/// The in-scope Indeo 5 `VisualSampleEntry` / `fccHandler` FourCC:
/// `IV50` (Indeo Video Interactive 5). Canonical upper-case spelling.
pub const INDEO5_FOURCCS: [&[u8; 4]; 1] = [b"IV50"];

/// Returns `Some(CodecId::new("indeo5"))` if `fourcc` (case-insensitive)
/// is the in-scope Indeo 5 FourCC (`IV50`).
///
/// A demuxer's `CodecResolver` calls this (or, equivalently, the
/// registry's tag path seeded by [`register_codecs`]) to route an `IV50`
/// video track to this crate.
pub fn codec_id_for_fourcc(fourcc: &[u8; 4]) -> Option<CodecId> {
    let mut upper = [0u8; 4];
    for i in 0..4 {
        upper[i] = fourcc[i].to_ascii_uppercase();
    }
    match &upper {
        b"IV50" => Some(CodecId::new(CODEC_ID_STR)),
        _ => None,
    }
}

/// The [`oxideav_core::registry::codec::DecoderFactory`] for Indeo 5:
/// construct a fresh [`Indeo5RegistryDecoder`] from the stream
/// parameters.
///
/// The parameters' `width` / `height` / `pixel_format` are advisory —
/// the Indeo 5 bitstream carries its own dimensions in the GOP header of
/// every intra frame, so the decoder is self-describing.
pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    Ok(Box::new(Indeo5RegistryDecoder::new(
        params.codec_id.clone(),
    )))
}

/// An [`oxideav_core`] [`Decoder`] backed by the in-crate stateful
/// [`Iv50Decoder`].
///
/// Holds the decoder, whose state spans frames, and a single-packet
/// pending slot. `send_packet` stashes the packet; `receive_frame`
/// decodes it and returns the picture as a [`VideoFrame`] whose layout
/// [`Decoder::output_params`] describes (see the module docs).
pub struct Indeo5RegistryDecoder {
    codec_id: CodecId,
    inner: Iv50Decoder,
    pending: Option<Packet>,
    eof: bool,
    /// Width, height and pixel format of the frame returned last.
    shape: Option<(u32, u32, PixelFormat)>,
    /// Moves when `shape` changes after the first frame.
    generation: u64,
    /// A decoded frame held back behind its [`Frame::StreamParams`]
    /// event.
    held: Option<VideoFrame>,
}

impl Indeo5RegistryDecoder {
    /// Create a fresh registry decoder for the given codec id.
    pub fn new(codec_id: CodecId) -> Self {
        Indeo5RegistryDecoder {
            codec_id,
            inner: Iv50Decoder::new(),
            pending: None,
            eof: false,
            shape: None,
            generation: 0,
            held: None,
        }
    }
}

/// Map an [`Iv50Error`] into the framework [`Error`] surface. Every
/// decode error means the packet is not a decodable IV50 frame at this
/// point of the stream.
fn map_decode_error(e: Iv50Error) -> Error {
    Error::invalid(format!("indeo5: {e}"))
}

/// Repeat each chroma sample over the 4 by 4 luma pixels it covers,
/// giving a plane of `width` by `height`. Luma pixel (x, y) takes chroma
/// sample (x / 4, y / 4): the chroma planes are `ceil(width / 4)` by
/// `ceil(height / 4)`.
fn upsample(p: &Iv50Plane, width: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = p.row((y / 4).min(p.height - 1));
        out.extend((0..width).map(|x| row[(x / 4).min(p.width - 1)]));
    }
    out
}

/// The visible samples of a plane, rows packed at the plane's width.
fn packed(p: &Iv50Plane) -> VideoPlane {
    VideoPlane {
        stride: p.width,
        data: (0..p.height)
            .flat_map(|r| p.row(r).iter().copied())
            .collect(),
    }
}

/// The pixel format of the decoder's current stream: [`PixelFormat::Yuv420P`]
/// when the chroma planes are half the luma size, [`PixelFormat::Yuv444P`]
/// (quarter-size chroma repeated 4 by 4) otherwise.
fn pixel_format_of(dec: &Iv50Decoder) -> Option<PixelFormat> {
    dec.chroma_ratio().map(|r| {
        if r == 2 {
            PixelFormat::Yuv420P
        } else {
            PixelFormat::Yuv444P
        }
    })
}

/// The decoder's current picture as a [`VideoFrame`] in Y, U, V order:
/// [`PixelFormat::Yuv420P`] planes as decoded for half-size chroma,
/// [`PixelFormat::Yuv444P`] (each plane at the luma size) for
/// quarter-size chroma.
fn picture_to_video_frame(dec: &Iv50Decoder, pts: Option<i64>) -> Result<VideoFrame> {
    let (Some(y), Some(u), Some(v)) = (dec.y(), dec.u(), dec.v()) else {
        return Err(Error::invalid("indeo5: no picture decoded yet"));
    };
    let (w, h) = (y.width, y.height);
    let planes = if dec.chroma_ratio() == Some(2) {
        vec![packed(&y), packed(&u), packed(&v)]
    } else {
        vec![
            packed(&y),
            VideoPlane {
                stride: w,
                data: upsample(&u, w, h),
            },
            VideoPlane {
                stride: w,
                data: upsample(&v, w, h),
            },
        ]
    };
    Ok(VideoFrame { pts, planes })
}

/// One-shot direct decode: decode a single Indeo 5 frame's bytes into an
/// [`oxideav_core`] [`VideoFrame`] (Y, U, V) without managing a
/// [`Decoder`] state machine. The planes are [`PixelFormat::Yuv444P`]
/// for a quarter-size-chroma stream and [`PixelFormat::Yuv420P`] for a
/// half-size-chroma one (the chroma planes' stride tells which).
///
/// It decodes `data` with a fresh [`Iv50Decoder`], so `data` must be an
/// intra frame; any other frame returns [`Error::invalid`]. Callers
/// decoding a sequence want [`Indeo5RegistryDecoder`] or
/// [`Iv50Decoder`] instead.
///
/// `pts` is carried straight onto the returned frame.
pub fn decode_video_frame(data: &[u8], pts: Option<i64>) -> Result<VideoFrame> {
    let mut decoder = Iv50Decoder::new();
    decoder.decode(data).map_err(map_decode_error)?;
    picture_to_video_frame(&decoder, pts)
}

impl Decoder for Indeo5RegistryDecoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::other(
                "indeo5 decoder: receive_frame must be called before sending another packet",
            ));
        }
        self.pending = Some(packet.clone());
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        if let Some(frame) = self.held.take() {
            return Ok(Frame::Video(frame));
        }
        let Some(pkt) = self.pending.take() else {
            return if self.eof {
                Err(Error::Eof)
            } else {
                Err(Error::NeedMore)
            };
        };
        self.inner.decode(&pkt.data).map_err(map_decode_error)?;
        let frame = picture_to_video_frame(&self.inner, pkt.pts)?;
        let shape = match (self.inner.dimensions(), pixel_format_of(&self.inner)) {
            (Some((w, h)), Some(format)) => Some((w as u32, h as u32, format)),
            _ => None,
        };
        if self.shape.is_some() && shape != self.shape {
            // A new shape mid-stream: the description and the counter
            // move with the event, the frame follows it.
            self.shape = shape;
            self.generation = self.generation.wrapping_add(1);
            self.held = Some(frame);
            let params = self.output_params().expect("shape known");
            return Ok(Frame::StreamParams(Box::new(params)));
        }
        self.shape = shape;
        Ok(Frame::Video(frame))
    }

    fn flush(&mut self) -> Result<()> {
        self.eof = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        // A container seek restarts the Indeo 5 inter-frame state: drop
        // the pending packet and the decoder, so the next packet must be
        // an intra frame. The shape of the last frame returned stays, so
        // a different one after the seek is announced like any other
        // change.
        self.pending = None;
        self.held = None;
        self.eof = false;
        self.inner = Iv50Decoder::new();
        Ok(())
    }

    fn output_pixel_format(&self) -> Option<PixelFormat> {
        self.output_params().and_then(|p| p.pixel_format)
    }

    fn output_params(&self) -> Option<CodecParameters> {
        let (width, height, format) = self.shape?;
        let mut p = CodecParameters::video(self.codec_id.clone());
        p.width = Some(width);
        p.height = Some(height);
        p.pixel_format = Some(format);
        Some(p)
    }

    fn output_params_generation(&self) -> u64 {
        self.generation
    }
}

/// Confidence [`probe`] returns when a first packet starts with valid
/// Indeo 5 headers.
pub const PROBE_CONFIDENCE_HEADER_OK: f32 = 1.0;

/// Confidence [`probe`] returns when no first-packet bytes are available
/// to validate (tags resolved at stream-discovery time, before any
/// packet is read). The FourCC match alone is decent evidence.
pub const PROBE_CONFIDENCE_TAG_ONLY: f32 = 0.6;

/// The [`oxideav_core::ProbeFn`] for Indeo 5 tag disambiguation.
///
/// When the demuxer has peeked a first packet ([`ProbeContext::packet`]
/// is `Some`), parse its picture start (start code 0x1f and a frame type
/// of 0 to 4), and for an intra frame its GOP header (subsampling code,
/// picture size, band layout) and frame header:
///
/// * headers that parse give [`PROBE_CONFIDENCE_HEADER_OK`], also when
///   they ask for a feature the decoder doesn't support;
/// * a packet whose headers don't parse gives `0.0`, so another claimant
///   of the FourCC can win on bytes that aren't Indeo 5;
/// * no packet gives [`PROBE_CONFIDENCE_TAG_ONLY`].
///
/// The probe reads only the headers, never the band data.
pub fn probe(ctx: &ProbeContext) -> f32 {
    let Some(bytes) = ctx.packet else {
        return PROBE_CONFIDENCE_TAG_ONLY;
    };
    if looks_like_iv50(bytes) {
        PROBE_CONFIDENCE_HEADER_OK
    } else {
        0.0
    }
}

/// Register the Indeo 5 decoder (id + capabilities + factory + probe +
/// the `IV50` FourCC tag) into a [`CodecRegistry`].
///
/// The capabilities advertise the lossy video codec emitting
/// [`PixelFormat::Yuv444P`] (quarter-size chroma, each sample repeated
/// over its 4 by 4 luma pixels) or [`PixelFormat::Yuv420P`] (half-size
/// chroma). No encoder is registered: the crate only decodes.
pub fn register_codecs(reg: &mut CodecRegistry) {
    let caps = CodecCapabilities::video("indeo5_sw")
        .with_lossy(true)
        .with_pixel_format(PixelFormat::Yuv444P)
        .with_pixel_format(PixelFormat::Yuv420P);
    reg.register(
        CodecInfo::new(CodecId::new(CODEC_ID_STR))
            .capabilities(caps)
            .decoder(make_decoder)
            .probe(probe)
            .tags([CodecTag::fourcc(b"IV50")]),
    );
}

/// Unified registration entry point: install the Indeo 5 codec factory
/// into the codec sub-registry of a [`RuntimeContext`].
pub fn register(ctx: &mut RuntimeContext) {
    register_codecs(&mut ctx.codecs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::stream::CodecResolver;

    #[test]
    fn chroma_sample_covers_four_by_four_luma_pixels() {
        // A 9 by 5 picture has 3 by 2 chroma samples; luma column x takes
        // chroma column x / 4, also when 9 / 3 would suggest a factor of 3.
        let data: Vec<u8> = (0..2 * 32)
            .map(|i| (i % 32) as u8 + 10 * (i / 32) as u8)
            .collect();
        let p = Iv50Plane {
            width: 3,
            height: 2,
            stride: 32,
            data: &data,
        };
        let up = upsample(&p, 9, 5);
        assert_eq!(&up[..9], &[0, 0, 0, 0, 1, 1, 1, 1, 2]);
        assert_eq!(&up[4 * 9..], &[10, 10, 10, 10, 11, 11, 11, 11, 12]);
    }

    #[test]
    fn codec_id_for_fourcc_maps_iv50() {
        assert_eq!(
            codec_id_for_fourcc(b"IV50"),
            Some(CodecId::new(CODEC_ID_STR))
        );
    }

    #[test]
    fn codec_id_for_fourcc_is_case_insensitive() {
        assert_eq!(
            codec_id_for_fourcc(b"iv50"),
            Some(CodecId::new(CODEC_ID_STR))
        );
        assert_eq!(
            codec_id_for_fourcc(b"Iv50"),
            Some(CodecId::new(CODEC_ID_STR))
        );
    }

    #[test]
    fn codec_id_for_fourcc_rejects_other_indeo_and_unrelated() {
        // Indeo 2 (RT21 / IV20), Indeo 3 (IV31 / IV32), Indeo 4 (IV41)
        // are separate codecs and must not resolve to the indeo5 id.
        assert_eq!(codec_id_for_fourcc(b"RT21"), None);
        assert_eq!(codec_id_for_fourcc(b"IV20"), None);
        assert_eq!(codec_id_for_fourcc(b"IV31"), None);
        assert_eq!(codec_id_for_fourcc(b"IV32"), None);
        assert_eq!(codec_id_for_fourcc(b"IV41"), None);
        assert_eq!(codec_id_for_fourcc(b"avc1"), None);
    }

    #[test]
    fn decoder_registered_no_encoder() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        assert!(reg.has_decoder(&CodecId::new(CODEC_ID_STR)));
        assert!(!reg.has_encoder(&CodecId::new(CODEC_ID_STR)));
    }

    #[test]
    fn register_via_runtime_context_installs_factory() {
        let mut ctx = RuntimeContext::new();
        register(&mut ctx);
        assert!(ctx.codecs.has_decoder(&CodecId::new(CODEC_ID_STR)));
    }

    #[test]
    fn registry_resolves_iv50_tag() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        let tag = CodecTag::fourcc(b"IV50");
        let ctx = ProbeContext::new(&tag);
        let id = reg.resolve_tag(&ctx).expect("resolve_tag");
        assert_eq!(id, CodecId::new(CODEC_ID_STR));
    }

    #[test]
    fn probe_no_packet_returns_tag_only_confidence() {
        let tag = CodecTag::fourcc(b"IV50");
        let ctx = ProbeContext::new(&tag);
        assert_eq!(probe(&ctx), PROBE_CONFIDENCE_TAG_ONLY);
    }

    #[test]
    fn probe_garbage_packet_returns_zero() {
        // A short all-zero packet cannot carry a valid picture-start
        // triplet (the PSC is 0x1f, not 0).
        let tag = CodecTag::fourcc(b"IV50");
        let junk = [0u8; 4];
        let ctx = ProbeContext::new(&tag).packet(&junk);
        assert_eq!(probe(&ctx), 0.0);
    }

    #[test]
    fn probe_accepts_real_frames_and_refuses_bad_headers() {
        let tag = CodecTag::fourcc(b"IV50");
        let frame: &[u8] = include_bytes!("../../tests/data/intra-320x240-indeo5.iv50");
        assert_eq!(
            probe(&ProbeContext::new(&tag).packet(frame)),
            PROBE_CONFIDENCE_HEADER_OK
        );
        // A predicted frame: picture start (type 1) and a frame header.
        let inter = [0x1f | (1 << 5), 7, 0x80, 0];
        assert_eq!(
            probe(&ProbeContext::new(&tag).packet(&inter)),
            PROBE_CONFIDENCE_HEADER_OK
        );
        // Frame type 5 and an intra frame whose subsampling code is 3.
        let bad_type = [0x1f | (5 << 5), 0, 0, 0];
        assert_eq!(probe(&ProbeContext::new(&tag).packet(&bad_type)), 0.0);
        // The fixture's gop_flags (byte 2) carry a 16-bit header size,
        // so the subsampling code is in bits 0..2 of byte 5.
        let mut bad_sub = frame.to_vec();
        bad_sub[5] = (bad_sub[5] & !7) | 3;
        assert_eq!(probe(&ProbeContext::new(&tag).packet(&bad_sub)), 0.0);
    }

    #[test]
    fn make_decoder_reports_codec_id() {
        let params = CodecParameters::video(CodecId::new(CODEC_ID_STR));
        let dec = make_decoder(&params).expect("make_decoder");
        assert_eq!(dec.codec_id(), &CodecId::new(CODEC_ID_STR));
    }

    #[test]
    fn receive_before_send_needs_more() {
        let mut dec = Indeo5RegistryDecoder::new(CodecId::new(CODEC_ID_STR));
        assert!(matches!(dec.receive_frame(), Err(Error::NeedMore)));
    }

    #[test]
    fn flush_then_receive_is_eof() {
        let mut dec = Indeo5RegistryDecoder::new(CodecId::new(CODEC_ID_STR));
        dec.flush().expect("flush");
        assert!(matches!(dec.receive_frame(), Err(Error::Eof)));
    }

    use crate::indeo5::decoder::testutil::{Band, Gop, Mb, frame};

    /// An intra frame of `width` by `height` with every macroblock
    /// skipped and empty chroma bands; `half` selects half-size chroma.
    fn flat_intra(width: u32, height: u32, half: bool) -> Vec<u8> {
        let gop = Gop {
            flags: if half { 2 } else { 0 },
            chroma: if half { 4 } else { 6 },
            ..Gop::simple(width, height)
        };
        let n = (width.div_ceil(16) * height.div_ceil(16)) as usize;
        let skip = Mb {
            skip: true,
            ..Default::default()
        };
        let luma = Band::new(true, vec![skip; n]);
        frame(0, 0, Some(&gop), 0, &[Some(&luma)], None, None)
    }

    fn packet(data: Vec<u8>, pts: i64) -> Packet {
        Packet {
            stream_index: 0,
            time_base: oxideav_core::TimeBase::new(1, 1000),
            pts: Some(pts),
            dts: Some(pts),
            duration: None,
            flags: Default::default(),
            data,
        }
    }

    fn shape_of(dec: &Indeo5RegistryDecoder) -> Option<(u32, u32, PixelFormat)> {
        let p = dec.output_params()?;
        assert_eq!(p.codec_id, CodecId::new(CODEC_ID_STR));
        assert_eq!(dec.output_pixel_format(), p.pixel_format);
        Some((p.width?, p.height?, p.pixel_format?))
    }

    /// Size and pixel format are stream metadata: unknown before the
    /// first frame, then the GOP header's; a change is one event before
    /// the first frame of the new shape, with the counter moved.
    #[test]
    fn stream_parameters_follow_the_gop_header() {
        let mut dec = Indeo5RegistryDecoder::new(CodecId::new(CODEC_ID_STR));
        assert_eq!(shape_of(&dec), None);
        assert_eq!(dec.output_params_generation(), 0);

        // First shape: 32x16, quarter-size chroma, emitted as 4:4:4.
        dec.send_packet(&packet(flat_intra(32, 16, false), 0))
            .unwrap();
        let Frame::Video(f) = dec.receive_frame().unwrap() else {
            panic!("the first shape is not announced by an event");
        };
        assert_eq!(
            f.planes.iter().map(|p| p.stride).collect::<Vec<_>>(),
            [32, 32, 32]
        );
        assert!(f.planes.iter().all(|p| p.data.len() == 32 * 16));
        assert_eq!(shape_of(&dec), Some((32, 16, PixelFormat::Yuv444P)));
        assert_eq!(dec.output_params_generation(), 0);

        // The same shape again: no event.
        dec.send_packet(&packet(flat_intra(32, 16, false), 1))
            .unwrap();
        assert!(matches!(dec.receive_frame(), Ok(Frame::Video(_))));
        assert_eq!(dec.output_params_generation(), 0);

        // Half-size chroma at the same size: the pixel format changes.
        dec.send_packet(&packet(flat_intra(32, 16, true), 2))
            .unwrap();
        let Frame::StreamParams(p) = dec.receive_frame().unwrap() else {
            panic!("expected the stream parameters event");
        };
        assert_eq!((p.width, p.height), (Some(32), Some(16)));
        assert_eq!(p.pixel_format, Some(PixelFormat::Yuv420P));
        assert_eq!(shape_of(&dec), Some((32, 16, PixelFormat::Yuv420P)));
        assert_eq!(dec.output_params_generation(), 1);
        let Frame::Video(f) = dec.receive_frame().unwrap() else {
            panic!("the frame follows its event");
        };
        assert_eq!(f.pts, Some(2));
        assert_eq!(
            f.planes.iter().map(|p| p.stride).collect::<Vec<_>>(),
            [32, 16, 16]
        );
        assert_eq!(
            f.planes.iter().map(|p| p.data.len()).collect::<Vec<_>>(),
            [32 * 16, 16 * 8, 16 * 8]
        );
        assert!(matches!(dec.receive_frame(), Err(Error::NeedMore)));

        // A new size with an odd half: chroma rounds up.
        dec.send_packet(&packet(flat_intra(18, 10, true), 3))
            .unwrap();
        assert!(matches!(dec.receive_frame(), Ok(Frame::StreamParams(_))));
        assert_eq!(shape_of(&dec), Some((18, 10, PixelFormat::Yuv420P)));
        assert_eq!(dec.output_params_generation(), 2);
        let Frame::Video(f) = dec.receive_frame().unwrap() else {
            panic!("the frame follows its event");
        };
        assert_eq!(
            f.planes.iter().map(|p| p.stride).collect::<Vec<_>>(),
            [18, 9, 9]
        );
        assert_eq!(f.planes[1].data.len(), 9 * 5);

        // A seek keeps the description; the same shape after it is not
        // a change, a different one is.
        dec.reset().unwrap();
        assert_eq!(shape_of(&dec), Some((18, 10, PixelFormat::Yuv420P)));
        dec.send_packet(&packet(flat_intra(18, 10, true), 4))
            .unwrap();
        assert!(matches!(dec.receive_frame(), Ok(Frame::Video(_))));
        assert_eq!(dec.output_params_generation(), 2);
        dec.reset().unwrap();
        dec.send_packet(&packet(flat_intra(32, 16, false), 5))
            .unwrap();
        assert!(matches!(dec.receive_frame(), Ok(Frame::StreamParams(_))));
        // A seek between the event and its frame drops the frame.
        dec.reset().unwrap();
        assert!(matches!(dec.receive_frame(), Err(Error::NeedMore)));
        assert_eq!(shape_of(&dec), Some((32, 16, PixelFormat::Yuv444P)));
        assert_eq!(dec.output_params_generation(), 3);
    }

    /// A frame that fails to decode changes nothing in the description.
    #[test]
    fn failed_frames_leave_the_stream_parameters() {
        let mut dec = Indeo5RegistryDecoder::new(CodecId::new(CODEC_ID_STR));
        dec.send_packet(&packet(flat_intra(32, 16, false), 0))
            .unwrap();
        dec.receive_frame().unwrap();
        let mut bad = flat_intra(64, 48, true);
        bad.truncate(bad.len() - 3);
        dec.send_packet(&packet(bad, 1)).unwrap();
        assert!(dec.receive_frame().is_err());
        assert_eq!(shape_of(&dec), Some((32, 16, PixelFormat::Yuv444P)));
        assert_eq!(dec.output_params_generation(), 0);
    }

    #[test]
    fn registry_built_decoder_starts_without_a_description() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        let params = CodecParameters::video(CodecId::new(CODEC_ID_STR));
        let dec = reg.first_decoder(&params).expect("registry decoder");
        assert!(dec.output_params().is_none());
        assert_eq!(dec.output_pixel_format(), None);
    }
}
