# Examples

Run these from the crate root. Each program has built-in input and explains its format and
buffer lifecycle in the source file.

| Example | Purpose | Run |
| --- | --- | --- |
| [records](records.rs) | Inspect LF-terminated byte records with bounded growth and lookbehind; optionally read a file | `cargo run --example records -- [file]` |
| [binary_frames](binary_frames.rs) | Read two-byte lengths followed by binary payloads, including empty frames | `cargo run --example binary_frames` |
| [retained_prefix](retained_prefix.rs) | Keep an owned header while copying its body through standard I/O | `cargo run --example retained_prefix` |

`[file]` is optional, not literal. `cargo test --examples` runs the examples' tests; the run
commands exercise the executable entry points.
