# Indeo test fixtures

## `intra-240x180-educ.iv50` / `intra-320x240-indeo5.iv50`

Real Intel/Ligos-encoded `IV50` INTRA keyframes (raw codec bitstream,
no AVI container), vendored from the clean-room docs staging at
`docs/video/indeo/indeo5/fixtures/` (r338, univdreams sandbox). They
are **media**, not source: the first `movi` chunk of
`Educ_Movie_DeadlyForce.avi` (240x180) and `indeo5.avi` (320x240) from
the project sample mirror. Full provenance (source URLs, container
SHAs, black-box reproduce commands against the vendor decoder) lives
in each fixture's `notes.md` under the docs staging directory.

| File | Bytes | SHA-256 |
| ---- | ----- | ------- |
| `intra-240x180-educ.iv50` | 260 | `c481126de9e74149f02f28e74693ddbd483e92e43e44c6b96ca7e54bf3922c2f` |
| `intra-320x240-indeo5.iv50` | 1632 | `f617e936dcdcdf76ccf18bf6219a175be8a798c3cea8627dfd47d21c6494e7bd` |

Both frames are `YVU9`, `decomp_levels = 0` (one band per plane),
single-tile-per-band. The 240x180 frame is a black frame (the vendor
decoder reproduces `Y=16, U=V=128` for it); the 320x240 frame carries
~1100 coded blocks across its three bands.
