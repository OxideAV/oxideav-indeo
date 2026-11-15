//! Criterion benchmarks for the Indeo 5 decoder over the staged `IV50`
//! fixtures under `tests/data/`:
//!
//! - `176x144_vendor_inter_8f`: the vendor-encoded sequence, one intra
//!   and seven inter frames (macroblock headers, vector deltas,
//!   inherited chroma vectors, motion compensation, residuals).
//! - `320x240_intra`: one intra keyframe with 1 105 coded blocks.
//! - `240x180_intra_flat`: one all-flat intra keyframe (headers, the
//!   uncoded-block fill and the plane writer).
//!
//! `iv50_decode` times [`Iv50Decoder::decode`] alone: the decoder is
//! built outside the timed region and decodes the whole sequence, so
//! the time covers header parsing, the band decode and the 8-bit
//! planes. `iv50_registry` times the `oxideav_core` bridge on the same
//! frames, which adds the copy of the planes into a `VideoFrame` (for
//! these quarter-size-chroma streams, with each chroma sample repeated
//! 4 by 4).
//!
//! Run with:
//!     cargo bench --bench indeo5_decode

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxideav_core::{CodecId, Decoder, Packet, TimeBase};
use oxideav_indeo::indeo5::{CODEC_ID_STR, Indeo5RegistryDecoder, Iv50Decoder};

const EDUC: &[u8] = include_bytes!("../tests/data/intra-240x180-educ.iv50");
const INDEO5: &[u8] = include_bytes!("../tests/data/intra-320x240-indeo5.iv50");
const INTER_BIN: &[u8] =
    include_bytes!("../tests/data/iv50-inter-176x144-8frame-vendor-enc/samples.bin");
const INTER_IDX: &str =
    include_str!("../tests/data/iv50-inter-176x144-8frame-vendor-enc/samples-index.csv");

/// The frames of the inter fixture, cut by its index
/// (`frame,offset,size,...`).
fn inter_frames() -> Vec<&'static [u8]> {
    INTER_IDX
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let (off, size): (usize, usize) = (f[1].parse().unwrap(), f[2].parse().unwrap());
            &INTER_BIN[off..off + size]
        })
        .collect()
}

fn sequences() -> Vec<(&'static str, Vec<&'static [u8]>)> {
    vec![
        ("176x144_vendor_inter_8f", inter_frames()),
        ("320x240_intra", vec![INDEO5]),
        ("240x180_intra_flat", vec![EDUC]),
    ]
}

fn decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("iv50_decode");
    for (name, frames) in sequences() {
        group.throughput(Throughput::Elements(frames.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &frames, |b, frames| {
            b.iter_batched(
                || {
                    // Sized and warmed up: the timed decodes reuse the
                    // buffers, as every frame after a stream's first.
                    let mut dec = Iv50Decoder::new();
                    dec.decode(frames[0]).unwrap();
                    dec
                },
                |mut dec| {
                    for f in frames {
                        std::hint::black_box(dec.decode(f).unwrap());
                    }
                    dec
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn registry(c: &mut Criterion) {
    let mut group = c.benchmark_group("iv50_registry");
    for (name, frames) in sequences() {
        let packets: Vec<Packet> = frames
            .iter()
            .enumerate()
            .map(|(i, f)| Packet {
                stream_index: 0,
                time_base: TimeBase::new(1, 30),
                pts: Some(i as i64),
                dts: Some(i as i64),
                duration: None,
                flags: Default::default(),
                data: f.to_vec(),
            })
            .collect();
        group.throughput(Throughput::Elements(frames.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &packets, |b, packets| {
            b.iter_batched(
                || {
                    let mut dec = Indeo5RegistryDecoder::new(CodecId::new(CODEC_ID_STR));
                    dec.send_packet(&packets[0]).unwrap();
                    dec.receive_frame().unwrap();
                    dec
                },
                |mut dec| {
                    for p in packets {
                        dec.send_packet(p).unwrap();
                        std::hint::black_box(dec.receive_frame().unwrap());
                    }
                    dec
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(benches, decode, registry);
criterion_main!(benches);
