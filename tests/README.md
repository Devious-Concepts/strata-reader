# Consumer contract coverage

Run `cargo test --test retained_stream` for composed public-API scenarios. They use only
`strata-reader` and the standard library, deterministic in-memory input, and no private helpers.
The fixture sizes are bounded; test-only indexing, arithmetic, and unwraps intentionally fail
when an asserted invariant breaks. These tests supplement the module tests and doctests rather
than reproduce every method case.

| Claim / contract | Evidence |
| --- | --- |
| Default capacity, builder alignment, maximum raised to initial capacity | `test_reader_new`, `test_reader_builder` in [reader unit tests](../src/reader/tests.rs); `test_additional_fills_and_capacity_alignment` |
| Single fill does not grow; multi-read growth is capped | `test_growth_compaction_and_explicit_reclamation`, `test_capacity_stop_is_not_eof_and_consumption_alone_does_not_free_space` |
| Consumed lookbehind persists through dynamic fills, and compaction resets its position without shrinking | `test_records_cross_reads_and_transfer_consumed_data`, `test_growth_compaction_and_explicit_reclamation` |
| Delimiters can span reads; fills may read ahead; EOF need not end a complete record | `test_records_cross_reads_and_transfer_consumed_data`, `test_long_record_grows_across_buffer_and_delimiter_boundaries` |
| Character/string fills handle split UTF-8 and reject invalid text; byte fills accept binary input | `test_utf8_delimiters_cross_reads_and_invalid_text_is_reported`; `test_buffer_as_str_from` in [buffer unit tests](../src/buffer/tests.rs) covers UTF-8 accessor boundaries |
| A zero fill count does not necessarily mean EOF | Capacity scenario, trait-object scenario, and the EOF assertion in the record scenario |
| Interrupts retry; non-exact fills retain partial progress on error | `test_partial_progress_survives_error_and_retry`, `test_non_exact_fills_retain_partial_progress`, `test_growth_before_error_is_retained_until_explicit_reclamation` |
| Exact-fill failure may consume source bytes without exposing the partial new input | `test_exact_fill_error_can_advance_the_source_without_exposing_new_bytes` |
| Amounts are additional bytes; at-least fills may over-read; requests beyond the cap fail | `test_additional_fills_and_capacity_alignment`; `test_reader_fill_amount`, `test_reader_fill_exact` |
| Ownership transfer preserves bytes and unread data independently of the reader | Record and handoff scenarios; `test_buffer_take_consumed` covers zero/all/some-consumed cases |
| Read, vectored read, string/end reads, and `BufRead` interoperate with retained data | Standard-I/O scenario plus `test_reader_read_*` and `test_reader_bufread_fill_buf` |
| Logical position accounts for read-ahead; relative seeks within retained bytes preserve the inner position; ordinary successful seeks clear data | `test_standard_io_and_relative_seek_respect_the_logical_position`; seek unit tests cover failures and boundary offsets |
| `DynamicReadExt` works on a trait object and sees only unconsumed data before reading | `test_dynamic_trait_object_checks_existing_unconsumed_data` |
| Growth rounding, shrink bounds, clear/discard, clone/equality, and technical capacity ceilings | `test_buffer_cap_*`, `test_buffer_grow_*`, `test_buffer_shrink_targeted`, `test_buffer_clear`, `test_buffer_clone`, `test_buffer_partial_eq`, and [constant tests](../src/constants/tests.rs) |

The tests establish observable byte, position, capacity, and error behavior. They do not count
allocator calls, prove a storage address remains stable during shrinking, or establish process
memory use or performance. `take_consumed()` transfers storage before shrinking it and creates a
replacement containing the unread suffix; there is no promise of zero allocation or zero copying.
This coverage map concerns library behavior, not historical release dates or licensing terms.
