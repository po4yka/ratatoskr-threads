## Why

XR-021 CONTRACTS.md R4-07: `CanonicalizedUrl::try_from` refuses any raw URL that contains a non-ASCII character or a percent sign before the query and fragment are cut off, while the browser extension routes on the pathname only. A shared Threads link with a percent-encoded or non-ASCII tracking parameter (`?xmt=a%3Db`, `?igshid=...`) is routed to the Threads lane and ends as a terminal `failed` / inaccessible operation instead of capturing a post the service could resolve.

## What Changes

- The ASCII and percent-escape checks apply to the authority and the path only. The query and the fragment are discarded unvalidated and never reach the canonical form.
- The 2048-character cap still bounds the whole raw input, and `original()` stays the input byte for byte.
- A path or handle that contains a percent sign or a non-ASCII character is still refused with `PercentEncoded` or `NonAscii`.
- **BREAKING**: an input that was refused with `PercentEncoded` or `NonAscii` only because of its query or fragment is now accepted. Nothing is deployed; there is no compatibility path.

## Capabilities

### Modified Capabilities

- `permalink-canonicalization`: states which parts of the input the ASCII and percent-escape rules apply to.

## Impact

- `crates/threads-archive/src/permalink.rs`, `crates/threads-archive/tests/permalink.rs`, `crates/threads-archive/tests/browser_capture_command.rs`.
- Every caller already goes through `CanonicalizedUrl` (explicit capture, the Platform command, public resolution, capture resolution), so none changes. No schema change and no wire contract change.
