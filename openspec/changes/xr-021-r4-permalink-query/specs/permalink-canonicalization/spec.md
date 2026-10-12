## MODIFIED Requirements

### Requirement: Every other input is refused with the violated rule named
The library SHALL refuse every input outside the documented grammar — a non-provider host or subdomain, a path without the `/@<handle>/post/<code>` shape (including profile URLs, missing segments, empty handle, or empty code), a syntactically invalid URL, and the network short form `/t/<code>` — with a typed error naming which rule failed, and SHALL NOT guess, repair beyond the documented normalization, or resolve anything over the network at intake time. The ASCII-only and no-percent-escape rules SHALL apply to the authority and the path only: the query string and the fragment SHALL be discarded without being validated, SHALL NOT appear in the canonical permalink, and SHALL NOT make an otherwise valid input fail. The length cap of 2048 characters SHALL bound the whole raw input, query and fragment included.

#### Scenario: A foreign host is refused by name
- **WHEN** a URL whose host is not one of the four documented provider hosts is submitted
- **THEN** canonicalization fails naming the host rule and produces no permalink

#### Scenario: A path without the post shape is refused
- **WHEN** inputs such as a bare profile URL, a path missing the `/post/` segment, or an empty handle or code are submitted
- **THEN** canonicalization fails naming the path-shape rule for each

#### Scenario: The short form is refused as unsupported at intake
- **WHEN** a `/t/<code>` short-form URL is submitted
- **THEN** canonicalization fails naming the form as unsupported at intake because resolving it requires the public-resolution lane, and produces no permalink

#### Scenario: A percent-encoded or non-ASCII query or fragment is discarded
- **WHEN** a valid permalink carries a query string or fragment containing a percent-escape or a non-ASCII character
- **THEN** canonicalization succeeds, yields the permalink without the query and fragment, and exposes the input byte for byte as the original

#### Scenario: A percent-escape or non-ASCII character in the authority or path is still refused
- **WHEN** the path, the handle or the authority contains a percent sign or a non-ASCII character
- **THEN** canonicalization fails naming the percent-encoding rule or the ASCII rule

#### Scenario: A long query is still bounded
- **WHEN** a valid permalink carries a query that pushes the whole input past 2048 characters
- **THEN** canonicalization fails naming the length rule
