# Releasing strata-reader

This procedure applies the
[Rust library verification recipe](https://github.com/viell-dev/strata-template/blob/main/ci/rust-library/README.md).
For contributor checks, see [CONTRIBUTING.md](CONTRIBUTING.md).

## 1. Prepare the release commit

In a pull request, as for any other change:

- Set `version` in `Cargo.toml`. If the release needs a newer compiler, raise `rust-version` too
  and note it in the changelog and the README.
- If `Cargo.lock` is tracked, run `cargo update --workspace` and include its changes in the
  release commit.
- Give the changelog entry its version, date, and link.
- Add anything new that should ship to the `include` list in `Cargo.toml`.

## 2. Run the gate

Check out the pull request's head commit and confirm that `git status --porcelain` prints
nothing; never use `--allow-dirty`. Plain `cargo` uses the development compiler that
`rust-toolchain.toml` selects. All three stages must pass. If CI has already run the first two
on this exact commit and passed, cite that run instead of repeating them.

### Fast checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

### Minimum supported Rust version

With a toolchain of exactly the version `rust-version` declares:

```bash
msrv=<rust-version>
rustup toolchain install "$msrv" --profile minimal
cargo +"$msrv" test --all-targets --all-features
cargo +"$msrv" test --doc --all-features
```

### Package

```bash
cargo package --list
cargo package
(cd target/package/strata-reader-<version> && cargo test --all-features)
```

The listing must contain every file the README, changelog, and licensing documents link to, and
every test, example, or benchmark meant to ship. If it does not, fix the `include` list in a new
commit and run the gate again. The tests in the extracted copy show that the published source is
testable as shipped.

Report the results in the pull request description: the commit, the host, the compiler versions,
the test counts per stage, and every ignored test with its reason. Then merge with a
fast-forward, so that the gated commit is the one that reaches `main`. A rebase after the gate
produces a new commit; gate that one instead.

## 3. Tag and publish

From the repository root, on the gated commit:

```bash
git tag <version>
git push origin <version>
cargo publish
```

Tags are the bare version, without a `v` prefix. Then:

- Check that the registry shows the intended version, README, license, and `rust-version`, and
  that docs.rs builds it.
- Create the forge release from the tag with the changelog entry as its body.
- Update anything that reports release status, such as an ecosystem hub, in its own repository.
