# oxideav-indeo

Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Round 1 — Indeo 3 (IV31 / IV32) frame-header parse.** This round
lands a typed parser for the combined 64-byte header that begins
every Indeo 3 codec frame (the 16-byte frame header plus the
48-byte bitstream header). The parser validates every constraint
the reference decoder enforces — `FRMH`-magic checksum,
`frame_size > 16`, `dec_version == 0x0020`, `YVU9_8BIT` rejection —
and surfaces every header field, the named `frame_flags` bits, the
NULL-frame `data_size == 0x80` sentinel, the signed `cb_offset`,
and the 16-byte `alt_quant[]` VQ-table-index table with helpers to
split each byte into its primary / secondary 4-bit nibbles.

No macroblock / VQ / motion-compensation decode yet — those layers
sit on the picture-layer chapters
(`docs/video/indeo/indeo3/spec/02-picture-layer.md` and later)
which haven't been authored yet. Indeo 2 / 4 / 5 currently have
only a multimedia.cx wiki snapshot under
`docs/video/indeo/indeoN/wiki/`, no `spec/` chapters, so they
remain at the round-0 scaffold pending docs work.

The previous (pre-orphan) implementation was retired alongside the
docs audit dated 2026-05-06 (see
[`AUDIT-2026-05-06.md`](https://github.com/OxideAV/docs/blob/master/AUDIT-2026-05-06.md));
the prior history is preserved on the `old` branch for archival
but is forbidden input for the rebuild.

## Quick start

```rust
use oxideav_indeo::indeo3::FrameHeader;

let frame: &[u8] = /* first 64+ bytes of an Indeo 3 codec frame */;
let header = FrameHeader::parse(frame)?;

if header.bitstream.is_null_frame() {
    // sync frame: reproduce output from prior-frame state
} else if header.bitstream.frame_flags.intra() {
    // key frame: decode the picture layer fresh
}
```

## Spec coverage

| Spec section                              | Covered |
| ----------------------------------------- | ------- |
| §2 frame header (16 B)                    | yes     |
| §2.1 `FRMH` checksum validation           | yes     |
| §2.2 `frame_size > 16` bound              | yes     |
| §3 bitstream header (48 B)                | yes     |
| §3.1 `dec_version == 0x0020`              | yes     |
| §3.2 `frame_flags` named bits             | yes     |
| §3.3 `data_size` + NULL-frame sentinel    | yes     |
| §3.4 signed `cb_offset`                   | surfaced |
| §3.5 bitstream `checksum` (read-only)     | surfaced |
| §3.6 `height` / `width` envelope          | surfaced |
| §3.7 Y / V / U plane offsets              | surfaced |
| §3.9 `alt_quant[16]` byte table + split   | yes     |
| §4 plane-decoder entry                    | deferred |
| §5 byte map                               | covered by tests |

"Surfaced" means the field is exposed verbatim on the typed
struct; the reference decoder does not validate the value, so we
do not either. "Deferred" means the work depends on later spec
chapters that aren't yet in `docs/`.

## Public API

* `oxideav_indeo::indeo3::FrameHeader::parse(&[u8])` — combined
  header decoder.
* `FrameHeaderPreamble`, `BitstreamHeader`, `FrameFlags`,
  `HeaderError`.
* Constants: `MAGIC_FRMH`, `REQUIRED_DEC_VERSION`,
  `FRAME_HEADER_LEN`, `BITSTREAM_HEADER_LEN`, `COMBINED_HEADER_LEN`,
  `FLAG_YVU9_8BIT`, `NULL_FRAME_DATA_SIZE_BITS`, `MIN_DIMENSION`,
  `MAX_WIDTH`, `MAX_HEIGHT`.
* `alt_quant_indices(byte) -> (primary, secondary)` for §3.9.

## License

MIT.
