//! Indeo 5 (`IV50`) registry-bridge end-to-end tests.
//!
//! Drive the real vendored IV50 INTRA keyframe through the framework's
//! published [`oxideav_core::Decoder`] surface (the
//! `indeo5::Indeo5RegistryDecoder` / `decode_video_frame` bridge) and
//! pin the shaped [`oxideav_core::VideoFrame`]: three equal-size
//! `Yuv444P` planes at the coded luma resolution, chroma box-upsampled
//! from the native 4:1:0 subsampling (`spec/08 §3.5`).

use oxideav_core::{CodecId, Decoder, Frame, Packet, TimeBase};
use oxideav_indeo::indeo5;

const INDEO5: &[u8] = include_bytes!("data/intra-320x240-indeo5.iv50");

fn packet(data: &[u8], pts: i64) -> Packet {
    Packet {
        stream_index: 0,
        time_base: TimeBase::new(1, 1000),
        pts: Some(pts),
        dts: Some(pts),
        duration: None,
        flags: Default::default(),
        data: data.to_vec(),
    }
}

#[test]
fn one_shot_decode_yields_yuv444p_at_luma_resolution() {
    let vf = indeo5::decode_video_frame(INDEO5, Some(9)).expect("one-shot decode");
    assert_eq!(vf.pts, Some(9));
    // Yuv444P: three planes, each full luma resolution (320x240).
    assert_eq!(vf.planes.len(), 3);
    for plane in &vf.planes {
        assert_eq!(plane.stride, 320);
        assert_eq!(plane.data.len(), 320 * 240);
    }
}

#[test]
fn registry_decoder_drives_send_receive() {
    let mut dec = indeo5::Indeo5RegistryDecoder::new(CodecId::new(indeo5::CODEC_ID_STR));
    dec.send_packet(&packet(INDEO5, 0)).expect("send");
    let frame = dec.receive_frame().expect("receive");
    match frame {
        Frame::Video(v) => {
            assert_eq!(v.pts, Some(0));
            assert_eq!(v.planes.len(), 3);
            assert_eq!(v.planes[0].data.len(), 320 * 240);
        }
        other => panic!("expected a video frame, got {other:?}"),
    }
    // No further frame until the next packet.
    assert!(matches!(
        dec.receive_frame(),
        Err(oxideav_core::Error::NeedMore)
    ));
}

#[test]
fn null_frame_repeats_previous_output_through_the_bridge() {
    let mut dec = indeo5::Indeo5RegistryDecoder::new(CodecId::new(indeo5::CODEC_ID_STR));
    dec.send_packet(&packet(INDEO5, 0)).expect("send intra");
    let Frame::Video(intra) = dec.receive_frame().expect("intra") else {
        panic!("expected video frame");
    };

    // A NULL frame: PSC (0x1f) + frame_type 4 + a fresh frame number.
    let null = [0x1f | (4 << 5), 0x01, 0, 0, 0, 0, 0, 0];
    dec.send_packet(&packet(&null, 1)).expect("send null");
    let Frame::Video(repeated) = dec.receive_frame().expect("null") else {
        panic!("expected video frame");
    };

    // The NULL frame re-emits the held pixels byte-for-byte, with its
    // own pts (spec/08 §6.4).
    assert_eq!(repeated.pts, Some(1));
    assert_eq!(repeated.planes.len(), intra.planes.len());
    for (a, b) in intra.planes.iter().zip(repeated.planes.iter()) {
        assert_eq!(a.data, b.data);
    }
}

#[test]
fn first_inter_packet_is_rejected() {
    // A non-INTRA first frame fails the session first-frame gate
    // (spec/01 §3.2), surfaced as invalid data through the bridge.
    let inter = [0x1f | (1 << 5), 0x00, 0, 0, 0, 0, 0, 0];
    assert!(matches!(
        indeo5::decode_video_frame(&inter, None),
        Err(oxideav_core::Error::InvalidData(_))
    ));
}

#[test]
fn reset_restarts_the_intra_gate() {
    let mut dec = indeo5::Indeo5RegistryDecoder::new(CodecId::new(indeo5::CODEC_ID_STR));
    dec.send_packet(&packet(INDEO5, 0)).expect("send");
    let _ = dec.receive_frame().expect("receive");
    dec.reset().expect("reset");
    // After reset the next packet is treated as the first frame again —
    // a fresh INTRA keyframe is accepted.
    dec.send_packet(&packet(INDEO5, 5))
        .expect("send post-reset");
    assert!(dec.receive_frame().is_ok());
}
