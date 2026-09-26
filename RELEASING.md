# Releasing strata-reader

How a release of this crate is verified, recorded, tagged, and published. The checks are the
Rust library recipe from [strata-template](https://codeberg.org/Devious-Concepts/strata-template);
this document orders them for a release and adds the evidence that is kept. Contributors do not
need any of this: the fast checks they run are listed in [CONTRIBUTING.md](CONTRIBUTING.md).

## 1. Prepare the release commit

In a pull request, as for any other change:

- Set `version` in `Cargo.toml`. Keep `rust-version` accurate: if this release needs a newer
  compiler, raise it there, verify that version in the gate below, and note the change in the
  changelog and the README.
- Give the changelog entry its version, the actual date, and its link.
- Compare the explicit `include` list in `Cargo.toml` with everything added since the last
  release: source modules, tests, examples, benchmarks, and helpers that should ship, plus every
  file the README, changelog, and licensing documents link to.
- Run the fast checks and report the results in the pull request.

Merge so that the release commit reaches `main` unchanged. The gate runs on the commit that will
be tagged; anything that rewrites it, such as a squash, moves the gate to the rewritten commit.

## 2. Check out the release commit cleanly

Use a fresh worktree of the exact commit. `cargo package` refuses a dirty working tree, and
`--allow-dirty` is not part of this procedure: a package built from uncommitted changes has no
commit to be traced back to.

```bash
git fetch origin
git worktree add --detach ../strata-reader-release origin/main
cd ../strata-reader-release
git status --porcelain   # prints nothing
git rev-parse HEAD       # the commit the record and the tag refer to
```

Remove the worktree when the release is done: `git worktree remove ../strata-reader-release`.

## 3. Run the gate

Three stages, all from the release worktree, all required to pass. Plain `cargo` uses the
development compiler that `rust-toolchain.toml` selects.

### Fast checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

Keep the doctest line: `--all-targets` does not include documentation tests.

### Minimum supported Rust version

Check the minimum this release declares, with a toolchain of exactly that version:

```bash
msrv=1.87.0   # the rust-version in Cargo.toml
rustup toolchain install "$msrv" --profile minimal
cargo +"$msrv" test --all-targets --all-features
cargo +"$msrv" test --doc --all-features
```

### Package

```bash
cargo package --list
cargo package
cat target/package/strata-reader-<version>/.cargo_vcs_info.json
sha256sum target/package/strata-reader-<version>.crate
cd target/package/strata-reader-<version>
cargo test --all-features
```

Read the listing before moving on. It must contain every file the README, changelog, and
licensing documents link to, and every test, example, or benchmark meant to ship; fix the
`include` list in a new release commit if it does not. `.cargo_vcs_info.json` must name the
release commit: it is what ties the published package to its source. It does not record whether
the tree was clean, which is why step 2 insists on a fresh worktree. Testing the extracted copy
shows that the published source is testable as shipped.

## 4. Record the results

Commit the record as `docs/releases/<version>.md`, after the tag, using this form. It states what
was tested and on what; the tests already ran, so keep it factual and short.

```markdown
# strata-reader <version> release record

| Field | Value |
| --- | --- |
| Commit | `<full commit hash>`, tag `<version>` |
| Checkout | Fresh worktree; `git status --porcelain` printed nothing |
| Host | <operating system, kernel, and architecture> |
| Development compiler | <`rustc --version` and `cargo --version`> |
| MSRV compiler | <`rustc +<msrv> --version`> |
| Gate run | <date>, by <who> |

## Results

| Stage | Result |
| --- | --- |
| Fast checks | <pass; test and doctest counts> |
| Minimum supported Rust version <msrv> | <pass; counts> |
| Package | <pass; `cargo test --all-features` counts in the extracted copy> |

Ignored tests: <each one with its reason and tracking issue, or "none">.

## Package

`cargo package --list`, <N> files:

<the listing>

`.cargo_vcs_info.json` names commit `<full commit hash>`. `strata-reader-<version>.crate`:
<size> bytes, SHA-256 `<digest>`.

## Publication

<registry version and its checksum, docs.rs build, forge release, and anything deferred>
```

## 5. Tag and publish

From the release worktree, still on the gated commit:

```bash
git tag -a <version> -m "strata-reader <version>"
git push origin <version>
cargo publish
```

Tags are the bare version, without a `v` prefix. `cargo publish` packages the commit again and
verifies it before uploading. Afterwards:

- Compare the registry's checksum for the version with the packaged file's digest. Check that the
  registry shows the intended version, README, license, and `rust-version`, and that docs.rs
  builds it.
- Create the forge release from the tag with the changelog entry as its body.
- Finish the record with the publication results and commit it.
- Update anything that reports release status, such as an ecosystem hub, in its own repository.

The procedure ends here. It does not sign tags or packages, and it does not automate publishing.
