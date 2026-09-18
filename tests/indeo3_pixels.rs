//! Indeo 3 real-bitstream pixel tests through the stateful picture
//! decoder (r459): both vendored `IV32` corpora decode **pixel-exact on
//! every frame and every plane** against their black-box reference
//! decodes (`expected.yuv`, planar `yuv410p`: Y, then U, then V) —
//! the all-intra 160×120 corpus (8 intra frames) and the 176×144
//! 4-frame-intra-period corpus (2 intra + 6 inter frames: motion
//! compensation with full- and half-pel vectors, the two-bank
//! reference ping-pong, the own-content families B / F).

use oxideav_core::{CodecId, Decoder, Frame, Packet, TimeBase};
use oxideav_indeo::indeo3::{decode_video_frame, Indeo3PictureDecoder, Indeo3RegistryDecoder};

const INTRA_BIN: &[u8] = include_bytes!("data/iv32-160x120-all-intra/samples.bin");
const INTRA_IDX: &str = include_str!("data/iv32-160x120-all-intra/samples-index.csv");
const INTRA_YUV: &[u8] = include_bytes!("data/iv32-160x120-all-intra/expected.yuv");
const INTER_BIN: &[u8] = include_bytes!("data/iv32-176x144-4frame-intra-period/samples.bin");
const INTER_IDX: &str = include_str!("data/iv32-176x144-4frame-intra-period/samples-index.csv");
const INTER_YUV: &[u8] = include_bytes!("data/iv32-176x144-4frame-intra-period/expected.yuv");

fn frames<'a>(bin: &'a [u8], idx: &str) -> Vec<&'a [u8]> {
    idx.lines()
        .skip(1)
        .map(|l| {
            let f: Vec<usize> = l.split(',').skip(1).map(|v| v.parse().unwrap()).collect();
            &bin[f[0]..f[0] + f[1]]
        })
        .collect()
}

/// Reference planes `(Y, U, V)` of frame `fi`.
fn reference(yuv: &[u8], fi: usize, w: usize, h: usize) -> (&[u8], &[u8], &[u8]) {
    let (cw, ch) = (w / 4, h / 4);
    let frame_len = w * h + 2 * cw * ch;
    let f = &yuv[fi * frame_len..(fi + 1) * frame_len];
    (
        &f[..w * h],
        &f[w * h..w * h + cw * ch],
        &f[w * h + cw * ch..],
    )
}

fn assert_sequence_exact(bin: &[u8], idx: &str, yuv: &[u8], w: usize, h: usize) {
    let mut dec = Indeo3PictureDecoder::new();
    for (fi, frame) in frames(bin, idx).iter().enumerate() {
        let pic = dec
            .decode(frame)
            .unwrap_or_else(|e| panic!("frame {fi}: {e}"));
        assert!(!pic.repeated_previous, "frame {fi}");
        assert_eq!((pic.width as usize, pic.height as usize), (w, h));
        let (y, u, v) = reference(yuv, fi, w, h);
        let exact = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(a, b)| a == b).count();
        assert_eq!(exact(&pic.luma, y), w * h, "frame {fi} luma");
        assert_eq!(exact(&pic.chroma_u, u), u.len(), "frame {fi} U");
        assert_eq!(exact(&pic.chroma_v, v), v.len(), "frame {fi} V");
        let stats = pic.stats.expect("picture frame carries stats");
        assert!(stats.iter().all(|s| s.cells > 0), "frame {fi}");
    }
}

#[test]
fn all_intra_160x120_eight_frames_pixel_exact() {
    assert_sequence_exact(INTRA_BIN, INTRA_IDX, INTRA_YUV, 160, 120);
}

#[test]
fn intra_period_176x144_eight_frames_pixel_exact() {
    assert_sequence_exact(INTER_BIN, INTER_IDX, INTER_YUV, 176, 144);
}

#[test]
fn inter_frames_reference_the_other_bank() {
    // Frames 1..3 and 5..7 are inter frames: every luma cell is an
    // INTER leaf on frame 1 and the residual families are the
    // own-content ones (B / F).
    let mut dec = Indeo3PictureDecoder::new();
    let frames = frames(INTER_BIN, INTER_IDX);
    let f0 = dec.decode(frames[0]).unwrap();
    let s0 = f0.stats.unwrap();
    assert_eq!(s0[0].inter, 0);
    assert_eq!(s0[0].families[0] + s0[0].families[4], s0[0].coded_cells);
    let f1 = dec.decode(frames[1]).unwrap();
    let s1 = f1.stats.unwrap();
    assert_eq!(s1[0].inter, s1[0].cells);
    assert_eq!(s1[0].families[1] + s1[0].families[5], s1[0].coded_cells);
    assert_eq!(s1[0].families[0], 0);
}

#[test]
fn registry_decoder_emits_pixel_exact_yuv444_frames() {
    let mut dec = Indeo3RegistryDecoder::new(CodecId::new("indeo3"));
    let (w, h) = (160usize, 120usize);
    for (fi, frame) in frames(INTRA_BIN, INTRA_IDX).iter().enumerate() {
        dec.send_packet(
            &Packet::new(0, TimeBase::new(1, 1000), frame.to_vec()).with_pts(fi as i64),
        )
        .unwrap();
        let Frame::Video(vf) = dec.receive_frame().unwrap() else {
            panic!("video frame")
        };
        assert_eq!(vf.pts, Some(fi as i64));
        assert_eq!(vf.planes.len(), 3);
        let (y, u, v) = reference(INTRA_YUV, fi, w, h);
        assert_eq!(vf.planes[0].data, y, "frame {fi} luma");
        // Chroma is box-replicated 4x4 (spec/07 §5.5).
        for (plane, r) in [(&vf.planes[1], u), (&vf.planes[2], v)] {
            assert_eq!(plane.stride, w);
            for yy in 0..h {
                for xx in 0..w {
                    assert_eq!(plane.data[yy * w + xx], r[(yy / 4) * (w / 4) + xx / 4]);
                }
            }
        }
    }
    // The one-shot direct API decodes the first frame identically.
    let vf = decode_video_frame(frames(INTRA_BIN, INTRA_IDX)[0], None).unwrap();
    assert_eq!(vf.planes[0].data, reference(INTRA_YUV, 0, w, h).0);
}
