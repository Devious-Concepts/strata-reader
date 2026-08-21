# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Buffer::take_consumed` and `Reader::take_consumed`, splitting off the
  consumed data as its own buffer, shortened and shrunk to the consumed size,
  while the unconsumed data is retained at the start of a fresh replacement
  buffer. Only the unconsumed bytes are copied; the existing allocation leaves
  with the consumed data.

## [0.1.1] - 2026-07-28

Documentation and licensing release; no code changes.

### Added

- `LICENSE-CLARIFICATION.md`, the maintainers' statement of intent on how
  MPL-2.0 applies to normal Rust usage (linking, monomorphization, inlining,
  macro expansion), private use, binary distribution, and generated code.
- MIT-0 licensing for example material: documentation code examples,
  `examples/`, `tests/`, and `benches/` may be copied freely under the new
  `LICENSE-MIT-0`, in addition to MPL-2.0.
- `CONTRIBUTING.md`, establishing an inbound=outbound contribution policy, a
  dual-license grant for example material, and contributor endorsement of the
  statement of intent.
- A pull request template recording each contributor's agreement to the
  contribution terms.

### Changed

- Rewrote the README license section to accurately describe MPL-2.0's
  file-level weak copyleft and the notice-and-source obligations of binary
  distribution.
- The published crate package now includes the licensing documents and the
  unit-test submodules, so the packaged source is complete and testable as
  shipped.

## [0.1.0] - 2026-05-06

Initial release of `strata-reader`, a `BufReader`-style wrapper with a
dynamically growing buffer and explicit memory control.

### Added

#### Core types

- `Buffer`, a standalone growable byte buffer with chunk-aligned capacity
  management. Capacity is always a multiple of `CHUNK_SIZE` (8 KiB) and grows
  in power-of-two steps up to a configured ceiling.
- `Reader<R>`, a `BufReader`-style wrapper around any `Read` source that
  delegates storage to a `Buffer` and enforces a configurable maximum
  capacity.
- `ReaderBuilder<R>`, returned from `Reader::builder`, for configuring
  `initial_capacity` and `max_capacity` before constructing a `Reader`. Both
  settings round up to the chunk alignment, and `max_capacity` is raised to
  match `initial_capacity` if the caller provides a smaller ceiling.
- `DynamicRead` trait, extending `BufRead` with buffer inspection and
  explicit memory management. Implemented for `Reader<R>`.
- `DynamicReadExt` trait, blanket-implemented for every `DynamicRead`
  implementor (including `&mut dyn DynamicRead`), providing the generic
  `fill_while` wrapper around the object-safe `fill_while_dyn` primitive.

#### Trait implementations on `Reader<R>`

- `Read`, including `read`, `read_exact`, `read_to_end`, and `read_to_string`.
- `BufRead`, including `fill_buf` and `consume`, with focused coverage of
  the single-read `fill_buf` semantics.
- `Seek`, with `stream_position` reporting the logical stream position and
  `seek` clearing the buffer after a successful seek of the inner reader.
- `Reader::seek_relative`, which can move within retained buffered data
  without touching the inner reader when the target lies inside the buffer.

#### Fill operations

- `fill`, performing a single underlying read into the buffer.
- `fill_amount`, a bounded at-least fill that reads until the buffer holds
  the requested number of unconsumed bytes or the source signals EOF.
- `fill_exact`, layered on top of `fill_amount` for callers that require an
  exact byte count.
- `fill_to_end`, reading until EOF subject to the maximum capacity.
- `fill_until(byte)`, `fill_until_char(ch)`, and `fill_until_str(needle)`,
  reading until a delimiter appears in the buffer.
- `fill_while(predicate)`, reading while a byte predicate continues to hold.
- All bounded fills use targeted growth: the buffer only grows enough to
  satisfy the request, never past `max_capacity`, and is not shrunk after a
  read error.

#### Inspection and peeking

- `buffer()`, `pos()`, and `capacity()` on `DynamicRead` for direct buffer
  inspection.
- `peek(n)`, returning up to _n_ unconsumed bytes without advancing the read
  position.
- `peek_behind(n)`, returning up to _n_ already-consumed bytes still
  retained in the buffer.
- UTF-8 string accessors with boundary alignment at the buffer end, so
  partial multi-byte sequences are not exposed as valid UTF-8.

#### Memory management

- `compact()` to reclaim consumed space by moving retained data to the front
  of the buffer.
- `shrink()` to release unused capacity back to chunk alignment.
- `clear()` and `discard()` to reset retained data, with documented
  semantics for each.
- `Reader::take_buffer` to move the underlying `Buffer` out of the reader.
- Automatic shrinking is suppressed after read errors to avoid discarding
  partially-read data.

#### Constants and limits

- `CHUNK_SIZE` (8 KiB) as the public chunk alignment.
- `DEFAULT_MAX_CAPACITY` (256 MiB) as the default ceiling for new readers.
- Internal `MAX_SUPPORTED_CAPACITY` and `MAX_EXPONENTIAL_CAPACITY` ceilings
  that clamp growth safely; these are technical limits, not tuning knobs.

#### Project metadata

- Crate-level documentation, README, and usage examples.
- `Cargo.toml` metadata including `repository`, `keywords`, `categories`,
  and a curated `include` list.
- Strict lint configuration: `unsafe_code = "deny"`, warn-level
  `clippy::pedantic` and `clippy::cargo`, and warnings on arithmetic,
  indexing, `unwrap`, and `expect`.
- Mozilla Public License 2.0.
- Minimum Supported Rust Version of **1.87.0** on Rust edition 2024.

[0.1.1]: https://codeberg.org/Devious-Concepts/strata-reader/commits/tag/0.1.1
[0.1.0]: https://codeberg.org/Devious-Concepts/strata-reader/commits/tag/0.1.0
