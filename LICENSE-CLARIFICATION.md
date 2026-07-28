# License Clarification: Statement of Intent

This document records the maintainers' intent and understanding regarding
the application of the Mozilla Public License, Version 2.0 ("MPL-2.0") to
this crate. It is not legal advice, and (apart from the separate MIT-0
license granted in "Example material" below) it does not modify, replace,
or supplement the terms of the license. If anything in this
document conflicts with the license text, the license text controls.

## Scope

"This crate" means the source files in this repository as published,
including releases distributed via crates.io. Except where a file expressly
states otherwise, those files are the "Covered Software" within the meaning
of MPL-2.0.

## 1. Private use carries no source-disclosure obligation

MPL-2.0's source-disclosure obligations (Sections 3.1 and 3.2) are
triggered by distributing Covered Software to others. Compiling this crate,
depending on it, modifying it for internal use, and running it as part of a
network service do not, by themselves, require you to publish or disclose
anything, so long as no copy, in source or executable form, is
distributed to another person or legal entity. Other license provisions,
such as the notice requirements of Section 3.4 and the termination terms of
Section 5, apply as written whenever their conditions are met.

## 2. Your own source files stay yours

Depending on this crate and compiling it into your project does not cause
your own source files to become Covered Software, provided those files
contain no material copied from this crate. In particular, the maintainers
do not consider any of the following to make your source files
Modifications of the Covered Software:

- static or dynamic linking against this crate;
- generic instantiation (monomorphization) of this crate's types and
  functions by your code;
- inlining of this crate's functions into your compiled artifacts;
- transient expansion of macros during compilation.

A new source file you write is Covered Software only if it contains
Covered Software copied from this crate, and not even then if the copied
material is separately licensed, as the example material below is.
Interoperating with the crate (implementing its traits, wrapping its
types, calling its functions) does not make a file covered. One caveat on
generated code: if a tool persists generated source that incorporates
MPL-licensed material from this crate (for example, checked-in
macro-expansion output), that generated source contains Covered Software
and remains under MPL-2.0 (Section 1.10(b)).

## 3. Compiled crate code stays covered

The compiled form of this crate inside your executable, library (`rlib`,
`staticlib`, `cdylib`, and similar), WebAssembly module, or firmware image
is the crate's "Executable Form" (Section 1.6) and remains Covered
Software. This does not restrict your own code in the same artifact:
Section 3.2(b) lets you distribute the Executable Form under the license of
your choice, provided that license does not attempt to limit or alter
recipients' rights in the Source Code Form, and Section 3.3 permits
combining Covered Software with other material in a Larger Work under
terms of your choosing. It does mean that if you distribute the artifact,
Section 3.2(a) requires you to:

- inform recipients that this crate is included and how to obtain its
  Source Code Form; and
- make that Source Code Form available under the terms of Section 3.1, by
  reasonable means, in a timely manner, at a charge no more than the cost
  of distribution to the recipient.

If you have not modified this crate, the maintainers consider giving
recipients a reference to the exact published crates.io release (name and
version), or to a corresponding immutable tag of this repository, a
reasonable means of satisfying Section 3.2(a), for as long as the complete
corresponding source remains publicly available there at no charge.

## 4. Modifications remain covered

Changes to this crate's files, and any new source file that contains
Covered Software copied from this crate (other than the separately
licensed example material), are Covered Software under MPL-2.0 and
are subject to its source-availability requirements when distributed
(Sections 3.1 and 3.2). Distributing this crate's source itself (vendored
copies, forks, patched versions, or republished archives) is source
distribution under Section 3.1.

## Example material

Separately from the statements above, the maintainers additionally license
the example material of this crate under the MIT No Attribution License
(MIT-0); see [LICENSE-MIT-0](LICENSE-MIT-0). Except where a file or an
individual fenced block expressly states otherwise, example material
means:

- the contents of fenced code blocks in this repository's Markdown files
  (including the README);
- the contents of fenced code blocks within Rust documentation comments
  (rustdoc) in the published source files;
- files under the `examples/` directory, if present; and
- files under the `tests/` directory (integration tests) and the
  `benches/` directory (benchmarks), if present.

Unit tests in `src/` are not example material and remain MPL-2.0 only.

Example material largely exists to show how to use the crate, so you may
copy it into your own projects without restriction and without incurring
MPL-2.0 obligations. Example material submitted by contributors is
licensed under both MPL-2.0 and MIT-0 per the policy in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Contributions

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution policy.
Contributions are accepted on terms that include each contributor's
confirmation that this statement of intent reflects their own intent for
their contribution.

## Provenance

This document is maintained as part of the
[strata-license](https://codeberg.org/Devious-Concepts/strata-license)
template. The copy in this repository is the version that applies to this
project.
