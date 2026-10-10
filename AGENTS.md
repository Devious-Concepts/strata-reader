# AGENTS.md

This file provides guidance to AI coding agents when working with code in this repository.
See `README.md` for project overview, features, usage, and MSRV; `CONTRIBUTING.md` for the fast
checks and contribution terms; and `RELEASING.md` for the release gate.

## Lint Configuration

The crate uses strict lints configured in `Cargo.toml`. Key points:

- `unsafe_code = "deny"`, unsafe is forbidden except with `#[expect(unsafe_code, reason = "...")]`
- `clippy::allow_attributes = "warn"` and `clippy::allow_attributes_without_reason = "warn"`,
  always use `#[expect(lint, reason = "...")]` instead of `#[allow(lint)]`
- Arithmetic, indexing, unwrap, and expect are all warned, use checked/saturating operations and
  document safety with `#[expect(..., reason = "...")]`
- The lints apply to every target. The benchmark and integration-test roots, and the examples'
  nested test modules, carry `#![expect(...)]` with a reason for the test-only allowances they need.

## Architecture

A released, essentially feature-complete single-crate library with four modules. All of it,
including `reader`, is the style reference for new code:

- **`buffer`**, `Buffer` struct: the standalone growable buffer with chunk-aligned capacity
  management; growth is exponential or linear depending on the operation. This is the core data
  structure.
- **`read`**, `DynamicRead` trait: extends `BufRead` with buffer inspection and memory management.
- **`reader`**, `Reader<R>` + `ReaderBuilder<R>`: wraps any `Read` with a `Buffer`,
  implements `Read`, `BufRead`, `DynamicRead`, and `Seek`.
- **`constants`**, `CHUNK_SIZE` (8 KiB), `DEFAULT_MAX_CAPACITY` (32 MiB), and the internal
  `MAX_SUPPORTED_CAPACITY` / `MAX_EXPONENTIAL_CAPACITY`. All buffer capacities are multiples of
  `CHUNK_SIZE`; the internal max constants are technical ceilings, not public tuning knobs.

Key design: `Buffer` does the heavy lifting; the `Reader` delegates to it while enforcing
`max_capacity` and providing the trait impls.

## Tests, Examples, and Benchmarks

- Unit tests live in nested `tests.rs` submodules per module (`src/<module>/tests.rs`).
- `tests/reader.rs` holds public-API scenarios in modules under `tests/reader/`, one per consumer
  concern; `tests/README.md` gives the run command. Scenario intent lives in source comments, not
  in a separate catalog.
- `examples/` holds three runnable programs, each with its tests in a nested `<name>/tests.rs`
  and `test = true` in `Cargo.toml`; `examples/README.md` indexes them. Each source file states
  its purpose, run command, sample output, input policy, and error behavior.
- `benches/reader.rs` is a Criterion harness with modules under `benches/reader/`, one per
  question. `benches/README.md` is the methodology, `benches/BASELINE.md` the measured results,
  `results.csv` the raw estimates, and `baseline.lock` the dependency resolution used. Validate
  outputs before timing; the cases also run in smoke mode under `cargo test --all-targets`.
- Test style everywhere: plain `fn test_*()` functions, explicit setup and actions, `.unwrap()`,
  state assertions, and short comments at each observable transition.
- Files under `examples/`, `tests/`, and `benches/` are example material under
  `LICENSE-CLARIFICATION.md` and ship in the package: keep the `include` list in `Cargo.toml`
  current. `Cargo.lock` is ignored; Criterion is the only dependency, for development only.

## CI

Both workflows run on the `home-ci` runner, which is not always online; queued jobs wait for it.
Keep each workflow's commands identical to its document when either changes.

- `.forgejo/workflows/merge-checks.yml` runs the fast checks from `CONTRIBUTING.md` on every
  pull request.
- `.forgejo/workflows/release-checks.yml` runs the three stages of `RELEASING.md` as separate
  jobs, by manual dispatch only. Pushes to `main` are not checked; dispatch it on `main` after
  merging when a full result is wanted.
