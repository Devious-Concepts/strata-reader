# AGENTS.md

This file provides guidance to AI coding agents when working with code in this repository.
See `README.md` for project overview, features, usage, and MSRV.

## Lint Configuration

The crate uses strict lints configured in `Cargo.toml`. Key points:

- `unsafe_code = "deny"` — unsafe is forbidden except with `#[expect(unsafe_code, reason = "...")]`
- `clippy::allow_attributes_without_reason = "warn"` — always use `#[expect(lint, reason = "...")]`
  instead of `#[allow(lint)]`
- Arithmetic, indexing, unwrap, and expect are all warned — use checked/saturating operations and
  document safety with `#[expect(..., reason = "...")]`

## Architecture

Single-crate library with four modules:

- **`buffer`** — `Buffer` struct: the standalone growable buffer with power-of-two-chunk capacity
  management. This is the core data structure.
- **`read`** — `DynamicRead` trait: extends `BufRead` with buffer inspection and memory management.
- **`reader`** — `Reader<R>` + `ReaderBuilder<R>`: wraps any `Read` with a `Buffer`,
  implements `Read`, `BufRead`, `DynamicRead`, and `Seek`.
- **`constants`** — `CHUNK_SIZE` (8 KiB), `DEFAULT_MAX_SIZE` (256 MiB), `PRACTICAL_MAX_SIZE`. All
  buffer capacities are multiples of `CHUNK_SIZE`.

Key design: `Buffer` does the heavy lifting; `Reader` delegates to it while enforcing
`max_capacity` and providing the trait impls. Tests live in sibling `tests.rs` files per module.
