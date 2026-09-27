# Integration tests

Run `cargo test --test reader`. The scenario modules under `reader/` explain their inputs,
operations, and expectations beside the assertions. They use only the public API and the
standard library; method-level tests remain under `src/`.
