# Releasing strata-reader

How a release is verified, tagged, published, and recorded. The checks are the Rust library
recipe from [strata-template](https://codeberg.org/Devious-Concepts/strata-template), run in
release order on the exact commit that is tagged. Contributors need only the fast checks, which
[CONTRIBUTING.md](CONTRIBUTING.md) lists.

## 1. Prepare the release commit

In a pull request, as for any other change:

- Set `version` in `Cargo.toml`. If the release needs a newer compiler, raise `rust-version` too
  and note it in the changelog and the README.
- Give the changelog entry its version, date, and link.
- Add anything new that should ship to the `include` list in `Cargo.toml`.

Merge it so that the commit reaches `main` unchanged. That commit is what the gate runs on and
what gets tagged.

## 2. Run the gate

Check out the release commit and confirm that `git status --porcelain` prints nothing; never use
`--allow-dirty`. Plain `cargo` uses the development compiler that `rust-toolchain.toml` selects.
All three stages must pass. If CI has already run the first two on this exact commit and passed,
cite that run in the record instead of repeating them.

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
cd target/package/strata-reader-<version>
cargo test --all-features
```

The listing must contain every file the README, changelog, and licensing documents link to, and
every test, example, or benchmark meant to ship. If it does not, fix the `include` list in a new
release commit and start over. The tests in the extracted copy show that the published source is
testable as shipped.

## 3. Tag and publish

From the same checkout, still on the gated commit:

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

## 4. Record the release

Add `docs/releases/<version>.md` in a pull request, using this form. Keep it factual and short: it
states what was tested, on what, and what was published.

```markdown
# strata-reader <version> release record

| Field | Value |
| --- | --- |
| Commit | `<full commit hash>`, tag `<version>` |
| Host | <operating system, kernel, and architecture> |
| Development compiler | <`rustc --version` and `cargo --version`> |
| MSRV compiler | <`rustc +<msrv> --version`> |
| Gate run | <date>, by <who> |

## Results

| Stage | Result |
| --- | --- |
| Fast checks | <pass; test and doctest counts, or the CI run> |
| Minimum supported Rust version <msrv> | <pass; counts, or the CI run> |
| Package | <pass; `cargo test --all-features` counts in the extracted copy> |

Ignored tests: <each one with its reason and tracking issue, or "none">.

## Package

`cargo package --list`, <N> files:

<the listing>

## Publication

<registry version, docs.rs build, forge release, and anything deferred>
```
