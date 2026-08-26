//! Permalink canonicalization contract for explicit capture intake (change
//! `add-explicit-capture`, tasks 1.1/1.2): every documented variant of one
//! permalink collapses to a single canonical value, identity folds handle case
//! while post codes stay verbatim, the original input survives byte-for-byte,
//! and every input outside the documented grammar is refused naming its rule.

use ratatoskr_threads_archive::permalink::{CanonicalizedUrl, PermalinkError};

/// The one canonical value every documented variant of the sample permalink
/// must produce: https scheme, `www.threads.net` host, folded handle, and the
/// verbatim post code.
const CANONICAL: &str = "https://www.threads.net/@user.name_x/post/AbC123xyz";

/// Run one input through canonicalization.
fn try_canonicalize(input: &str) -> Result<CanonicalizedUrl, PermalinkError> {
    CanonicalizedUrl::try_from(input)
}

/// Require that one input is refused and hand back why.
#[expect(
    clippy::expect_used,
    reason = "test helper outside any single test fn: an unexpected acceptance is the failure"
)]
fn refusal(input: &str) -> PermalinkError {
    try_canonicalize(input).expect_err("input outside the documented grammar must be refused")
}

#[test]
fn every_documented_variant_of_one_permalink_canonicalizes_to_the_same_value() {
    let variants = [
        "https://www.threads.net/@User.Name_x/post/AbC123xyz",
        "https://threads.net/@User.Name_x/post/AbC123xyz",
        "https://www.threads.com/@User.Name_x/post/AbC123xyz",
        "https://threads.com/@User.Name_x/post/AbC123xyz",
        "http://threads.com:443/@User.Name_x/post/AbC123xyz",
        "https://threads.net:443/@User.Name_x/post/AbC123xyz",
        "https://www.threads.net/@User.Name_x/post/AbC123xyz?igsh=x&y=z#f",
        "https://www.threads.net/@User.Name_x/post/AbC123xyz/",
    ];
    for variant in variants {
        let canonicalized =
            try_canonicalize(variant).expect("every documented variant must canonicalize");
        assert_eq!(
            canonicalized.permalink().as_str(),
            CANONICAL,
            "variant {variant} must yield exactly the single canonical permalink"
        );
    }
}

#[test]
fn handle_case_never_changes_identity() {
    let upper = try_canonicalize("https://www.threads.net/@JohnDoe/post/XyZ1")
        .expect("a mixed-case handle must canonicalize");
    let lower = try_canonicalize("https://www.threads.net/@johndoe/post/XyZ1")
        .expect("an already-lowercase handle must canonicalize");
    assert_eq!(
        upper.permalink(),
        lower.permalink(),
        "handle spelling case must not change identity"
    );
    assert_eq!(
        upper.permalink().as_str(),
        "https://www.threads.net/@johndoe/post/XyZ1"
    );
}

#[test]
fn the_post_code_is_preserved_verbatim() {
    let mixed = try_canonicalize("https://www.threads.net/@user/post/ABCdef")
        .expect("a mixed-case post code must canonicalize");
    let lower = try_canonicalize("https://www.threads.net/@user/post/abcdef")
        .expect("a lowercase post code must canonicalize");
    assert_ne!(
        mixed.permalink(),
        lower.permalink(),
        "post codes are case-sensitive provider tokens; folding them could merge distinct posts"
    );
    assert_eq!(
        mixed.permalink().as_str(),
        "https://www.threads.net/@user/post/ABCdef"
    );
    assert_eq!(
        lower.permalink().as_str(),
        "https://www.threads.net/@user/post/abcdef"
    );
}

#[test]
fn the_original_input_survives_alongside_the_canonical_value() {
    let input = "http://threads.com:80/@User.Name_x/post/AbC123xyz?igsh=abc#frag";
    let canonicalized = try_canonicalize(input).expect("the input must canonicalize");
    assert_eq!(canonicalized.original(), input);
    assert_eq!(canonicalized.permalink().as_str(), CANONICAL);
}

#[test]
fn a_foreign_host_is_refused_naming_the_host_rule() {
    assert_eq!(
        refusal("https://example.com/@user/post/abc"),
        PermalinkError::Host
    );
}

#[test]
fn lookalike_subdomain_and_suffix_hosts_are_refused_naming_the_host_rule() {
    assert_eq!(
        refusal("https://evil.threads.net/@user/post/abc"),
        PermalinkError::Host,
        "a subdomain of a provider host is still a foreign host"
    );
    assert_eq!(
        refusal("https://threads.net.evil.com/@user/post/abc"),
        PermalinkError::Host,
        "a provider name used as a suffix is still a foreign host"
    );
}

#[test]
fn a_bare_profile_url_is_refused_naming_the_path_shape_rule() {
    assert_eq!(
        refusal("https://www.threads.net/@user"),
        PermalinkError::PathShape
    );
}

#[test]
fn a_path_missing_the_post_segment_is_refused_naming_the_path_shape_rule() {
    assert_eq!(
        refusal("https://www.threads.net/@user/reply/X"),
        PermalinkError::PathShape
    );
}

#[test]
fn a_path_without_a_handle_is_refused_naming_the_path_shape_rule() {
    assert_eq!(
        refusal("https://www.threads.net/post/X"),
        PermalinkError::PathShape
    );
}

#[test]
fn an_empty_handle_is_refused_by_the_handle_grammar_rule() {
    assert_eq!(
        refusal("https://www.threads.net/@/post/X"),
        PermalinkError::HandleGrammar
    );
}

#[test]
fn an_empty_post_code_is_refused_by_the_post_code_grammar_rule() {
    assert_eq!(
        refusal("https://www.threads.net/@user/post/"),
        PermalinkError::PostCodeGrammar
    );
}

#[test]
fn a_non_http_scheme_is_refused_naming_the_scheme_rule() {
    assert_eq!(
        refusal("ftp://www.threads.net/@user/post/abc"),
        PermalinkError::Scheme
    );
}

#[test]
fn a_schemeless_url_is_refused_as_an_invalid_url() {
    assert_eq!(
        refusal("www.threads.net/@a/post/b"),
        PermalinkError::InvalidUrl
    );
    let with_scheme_added = try_canonicalize("https://www.threads.net/@a/post/b")
        .expect("restoring the explicit scheme must make the same URL acceptable");
    assert_eq!(
        with_scheme_added.permalink().as_str(),
        "https://www.threads.net/@a/post/b",
        "the refusal above must be caused by the missing scheme alone"
    );
}

#[test]
fn a_relative_path_is_refused_as_an_invalid_url() {
    assert_eq!(refusal("/@a/post/b"), PermalinkError::InvalidUrl);
    let absolute = try_canonicalize("https://www.threads.net/@a/post/b")
        .expect("an absolute URL carrying the same path must be acceptable");
    assert_eq!(
        absolute.permalink().as_str(),
        "https://www.threads.net/@a/post/b",
        "the refusal above must be caused by the missing authority alone"
    );
}

#[test]
fn a_non_default_port_is_refused_naming_the_port_rule() {
    assert_eq!(
        refusal("https://www.threads.net:8080/@user/post/abc"),
        PermalinkError::Port
    );
}

#[test]
fn a_percent_encoded_handle_is_refused_naming_the_percent_encoding_rule() {
    assert_eq!(
        refusal("https://www.threads.net/@%41bc/post/abc"),
        PermalinkError::PercentEncoded
    );
}

#[test]
fn a_non_ascii_handle_is_refused_naming_the_ascii_rule() {
    assert_eq!(
        refusal("https://www.threads.net/@üserr/post/abc"),
        PermalinkError::NonAscii
    );
}

#[test]
fn an_input_over_the_length_cap_is_refused_naming_the_length_rule() {
    let mut long = String::from("https://www.threads.net/@user/post/abc?");
    while long.len() < 2049 {
        long.push('x');
    }
    assert_eq!(
        long.len(),
        2049,
        "the probe input must sit just past the cap"
    );
    assert_eq!(refusal(&long), PermalinkError::TooLong(2049));
}

#[test]
fn the_t_short_form_is_refused_as_unsupported_at_intake() {
    let error = refusal("https://www.threads.net/t/AbC123");
    assert_eq!(error, PermalinkError::ShortFormUnsupported);
    let message = error.to_string();
    assert!(
        message.contains("unsupported at intake") && message.contains("public-resolution"),
        "the short-form refusal must state that resolution belongs to the \
         public-resolution lane: {message}"
    );
}

#[test]
fn handles_up_to_thirty_characters_are_accepted_and_longer_refused() {
    let at_limit = "abcdefghijklmnopqrstuvwxyz__._";
    let url = format!("https://WWW.THREADS.NET/@{at_limit}/post/xyz");
    let canonicalized =
        try_canonicalize(&url).expect("a 30-character handle sits at the grammar limit");
    assert_eq!(
        canonicalized.permalink().as_str(),
        format!("https://www.threads.net/@{at_limit}/post/xyz")
    );

    let mut over_limit = String::from(at_limit);
    over_limit.push('x');
    assert_eq!(
        refusal(&format!("https://www.threads.net/@{over_limit}/post/xyz")),
        PermalinkError::HandleGrammar
    );
}

#[test]
fn post_codes_up_to_128_characters_are_accepted_and_longer_refused() {
    let at_limit = format!("{}ab", "AbC123".repeat(21));
    let canonicalized = try_canonicalize(&format!("https://threads.com/@user/post/{at_limit}"))
        .expect("a 128-character post code sits at the grammar limit");
    assert_eq!(
        canonicalized.permalink().as_str(),
        format!("https://www.threads.net/@user/post/{at_limit}")
    );

    let over_limit = format!("{at_limit}x");
    assert_eq!(
        refusal(&format!("https://threads.com/@user/post/{over_limit}")),
        PermalinkError::PostCodeGrammar
    );
}
