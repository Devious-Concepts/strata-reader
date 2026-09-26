# Contributing

Thank you for your interest in contributing to `strata-reader`. Issues and pull requests are
welcome on the [upstream repository](https://codeberg.org/Devious-Concepts/strata-reader).

## Licensing of contributions

This project uses inbound=outbound licensing: any contribution you intentionally submit for
inclusion in this repository is licensed under the
[Mozilla Public License, Version 2.0](LICENSE), without additional terms or conditions of your
own; exceptions require prior written agreement from the maintainers. By contributing, you
represent that the contribution is your original creation, or that you have sufficient rights to
submit it under these terms (MPL-2.0, Section 2.5).

Two further points are conditions of contributing to this project:

- **Example material is dual-licensed.** By submitting material within the example scope defined
  in [LICENSE-CLARIFICATION.md](LICENSE-CLARIFICATION.md) (fenced code-block examples in
  Markdown and rustdoc, and files under `examples/`, `tests/`, and `benches/`), you license
  that contribution under both MPL-2.0 and [MIT-0](LICENSE-MIT-0).
- **You endorse the statement of intent.** [LICENSE-CLARIFICATION.md](LICENSE-CLARIFICATION.md)
  is the project's published statement of how the maintainers understand MPL-2.0 to apply to
  this crate. By contributing, you confirm that the version of that statement in effect when
  your contribution is accepted also reflects your intent with respect to your contribution. In
  particular, you agree, for the benefit of downstream recipients of this crate, not to
  assert that static or dynamic linking, monomorphization, inlining, or transient macro
  expansion, without copying MPL-only material into downstream source files, causes those files
  to become Covered Software or Modifications under MPL-2.0.

Contributions are merged only under these conditions. If you cannot accept them, please open an
issue to discuss before submitting; any agreed exception will be recorded explicitly in the
repository. So that acceptance is recorded, include the following sentence in your pull request
description:

> I have read and agree to the contribution terms in CONTRIBUTING.md.

## Development

- The minimum supported Rust version is declared by `rust-version` in `Cargo.toml` and stated in
  the README. [`rust-toolchain.toml`](rust-toolchain.toml) selects the development compiler,
  which may be newer; it is not the MSRV.
- The crate uses strict lints configured in `Cargo.toml`. Prefer
  `#[expect(lint, reason = "...")]` over `#[allow(lint)]`, and document safety around
  arithmetic, indexing, and unwrapping.
- Unit tests live in nested `tests.rs` submodules per module (`src/<module>/tests.rs`).
- Before submitting, run the fast checks with the development compiler:

  ```bash
  cargo fmt --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features
  cargo test --doc --all-features
  RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
  ```

  Releases also check the declared minimum compiler and the packaged source; that procedure is
  in [RELEASING.md](RELEASING.md).

## Provenance

This document is maintained as part of the
[strata-template](https://codeberg.org/Devious-Concepts/strata-template) template. The copy in
this repository is the version that applies to this project.
