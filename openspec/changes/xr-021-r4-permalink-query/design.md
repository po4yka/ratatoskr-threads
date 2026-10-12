## Context

See [proposal.md](proposal.md) and XR-021 CONTRACTS.md R4-07. The extension classifies on the pathname of a link; the service used to validate the raw text, so a tracking parameter with an escape was accepted by the extension and refused by the service.

## Decisions

**Cut first, then check.** The scheme is split off, then the fragment and the query are cut (the same cut the code already made), and the `NonAscii` and `PercentEncoded` checks run on what remains: the authority and the path. The error order for an input that is wrong in several ways changes only in that a defect in the query or fragment is no longer an error at all; the length check stays first and on the whole raw input, so a hostile long query is still bounded.

**No new caller work.** `capture.rs`, `browser_capture_command.rs`, `public_resolution.rs` and `capture_resolution.rs` all call `CanonicalizedUrl::try_from`; the command test proves the real capture path records the capture. The callers that re-parse a stored canonical URL see only canonical text and are unaffected.

**Scheme split stays ahead of the checks.** An input without `://` is still `InvalidUrl` and a foreign scheme is still `Scheme`; a non-ASCII or percent character in the scheme part is such a case and keeps being refused by those rules.

## Risks / Trade-offs

- An input with a non-ASCII byte before `://` now reports `InvalidUrl` or `Scheme` instead of `NonAscii`; both are refusals, and the typed rule names the first violated rule of the new order.
