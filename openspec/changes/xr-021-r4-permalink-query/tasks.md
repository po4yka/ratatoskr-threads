## 1. The grammar applies to the authority and path only (R4-07)

- [x] 1.1 RED: in `crates/threads-archive/tests/permalink.rs` add `a_percent_encoded_query_is_discarded_not_refused`, `a_non_ascii_query_is_discarded_not_refused`, `a_percent_encoded_fragment_is_discarded_not_refused`, `an_over_long_query_is_still_too_long`, `a_percent_escape_in_the_path_is_still_refused` and `a_non_ascii_byte_in_the_authority_or_path_is_still_refused`; the first three fail today with `PercentEncoded` or `NonAscii`.
- [x] 1.2 GREEN: cut the fragment and the query before the `NonAscii` and `PercentEncoded` checks in `CanonicalizedUrl::try_from`.

## 2. The capture command reaches the resolution lane (R4-07)

- [x] 2.1 RED: `crates/threads-archive/tests/browser_capture_command.rs::a_command_whose_permalink_carries_a_percent_encoded_query_is_recorded_for_resolution` delivers a command whose permalink has `?xmt=a%3Db` and expects a queued capture row for the canonical permalink and the queued report; it fails today because the command is refused.
- [x] 2.2 GREEN: nothing beyond 1.2 if every caller goes through `CanonicalizedUrl`; the test passes once 1.2 lands.

## 3. Documentation

- [x] 3.1 Documentation, no failing test can precede it: `openspec validate --all --strict` passes with the modified requirement.
