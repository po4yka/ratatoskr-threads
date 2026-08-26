# permalink-canonicalization Specification

## Purpose
Defines which textual Threads URL forms explicit capture accepts, the one stable canonical permalink each accepted form produces, and which forms are refused. Canonicalization is purely textual: no network request belongs to it.

## Requirements

### Requirement: Documented post-URL forms canonicalize to one stable permalink
The library SHALL accept exactly the textual Threads post-permalink forms `<scheme>://<host>/@<handle>/post/<code>` where scheme is `http` or `https`, host is `threads.net`, `www.threads.net`, `threads.com`, or `www.threads.com` (with any explicit default port accepted), handle matches the provider handle grammar of ASCII letters, digits, periods, and underscores, and the post code is preserved verbatim. Every accepted form SHALL produce exactly one canonical value, `https://www.threads.net/@<handle>/post/<code>` with the handle lowercased: scheme upgraded to `https`, host normalized to `www.threads.net`, default port dropped, query string and fragment dropped, trailing slash dropped, and handle case folded. The canonicalization result SHALL also carry the original input text unchanged.

#### Scenario: Every documented variant of one permalink canonicalizes to the same value
- **WHEN** each variant in the documented table — apex and www hosts on both provider domains, an `http` input with an explicit `:443` port, a URL carrying a tracking query string and fragment, and one with a trailing slash — is canonicalized
- **THEN** every row yields exactly `https://www.threads.net/@<handle>/post/<code>` for that permalink

#### Scenario: Handle case never changes identity
- **WHEN** two inputs differ only in handle letter case
- **THEN** both canonicalize to the identical lowercase permalink

#### Scenario: The post code is preserved verbatim
- **WHEN** two inputs differ only in post-code letter case
- **THEN** each canonical output preserves its own input code unchanged, so the two permalinks stay distinct

#### Scenario: The original input survives alongside the canonical value
- **WHEN** any input is canonicalized
- **THEN** the result exposes the original input text byte-for-byte next to the canonical permalink

### Requirement: Every other input is refused with the violated rule named
The library SHALL refuse every input outside the documented grammar — a non-provider host or subdomain, a path without the `/@<handle>/post/<code>` shape (including profile URLs, missing segments, empty handle, or empty code), a syntactically invalid URL, and the network short form `/t/<code>` — with a typed error naming which rule failed, and SHALL NOT guess, repair beyond the documented normalization, or resolve anything over the network at intake time.

#### Scenario: A foreign host is refused by name
- **WHEN** a URL whose host is not one of the four documented provider hosts is submitted
- **THEN** canonicalization fails naming the host rule and produces no permalink

#### Scenario: A path without the post shape is refused
- **WHEN** inputs such as a bare profile URL, a path missing the `/post/` segment, or an empty handle or code are submitted
- **THEN** canonicalization fails naming the path-shape rule for each

#### Scenario: The short form is refused as unsupported at intake
- **WHEN** a `/t/<code>` short-form URL is submitted
- **THEN** canonicalization fails naming the form as unsupported at intake because resolving it requires the public-resolution lane, and produces no permalink
