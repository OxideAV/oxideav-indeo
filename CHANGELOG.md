# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial scaffold of the `oxideav-indeo` crate covering the whole
  Intel Indeo video codec family in a single crate.
- Round 1: Indeo 2 (`RT21` / `IV20`, codec id `indeo2`) — frame-header
  parser per `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`,
  bit-reader for the entropy payload, and a stub plane-decode path that
  emits a structurally correct `yuv420p` frame at the dimensions
  declared in the bitstream. Static codeword-length / delta tables are
  not yet derived; full pair / run entropy decode will land in a
  follow-up round.
- Module layout (`v2`, future `v3`/`v4`/`v5`) and shared `common`
  helpers ready for the next rounds.
