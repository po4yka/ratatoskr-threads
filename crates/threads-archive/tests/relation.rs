//! Relation-contract tests: the open relation-kind grammar aligned with the
//! published `SocialRelationKind` token, explicit direction, provider-id
//! targeting, and unresolved targets preserved instead of dropped.
//!
//! The grammar pinned here is copied from `ratatoskr-contracts`
//! `crates/social-contracts/src/relation.rs` at revision `fb88f94`
//! (2026-08-25), recorded in `docs/CAPABILITY_MATRIX.md`.

use ratatoskr_threads_archive::relation::{
    PostRelation, RelationKind, RelationKindError, RelationTarget, ResolvedTarget, UnresolvedTarget,
};

#[test]
fn documented_relation_kinds_parse_and_round_trip() {
    for token in ["reply", "quote", "repost"] {
        let kind = RelationKind::try_from(token).unwrap();
        assert_eq!(kind.as_str(), token, "kind must round-trip unchanged");
        assert_eq!(kind.to_string(), token, "display must be the wire value");
    }
}

#[test]
fn an_unknown_well_formed_kind_is_preserved() {
    let mention = RelationKind::try_from("mention")
        .expect("a well-formed unknown kind must parse under the open grammar");

    assert_eq!(mention.as_str(), "mention", "unknown kinds round-trip");
    for documented in ["reply", "quote", "repost"] {
        assert_ne!(
            mention.as_str(),
            documented,
            "an unknown kind is distinguishable from every documented kind"
        );
    }
}

#[test]
fn a_malformed_kind_is_refused_naming_the_violated_rule() {
    assert_eq!(
        RelationKind::try_from("").unwrap_err(),
        RelationKindError::Empty,
        "the empty token names its rule"
    );
    assert_eq!(
        RelationKind::try_from("Mention").unwrap_err(),
        RelationKindError::LeadingCharacter('M'),
        "uppercase starts name their character"
    );
    assert_eq!(
        RelationKind::try_from("1mention").unwrap_err(),
        RelationKindError::LeadingCharacter('1'),
        "digits may not start a token"
    );
    assert_eq!(
        RelationKind::try_from("_mention").unwrap_err(),
        RelationKindError::LeadingCharacter('_'),
        "underscores may not start a token"
    );
    assert_eq!(
        RelationKind::try_from("me ntion").unwrap_err(),
        RelationKindError::IllegalCharacter(' '),
        "spaces are outside the grammar"
    );

    let too_long = format!("{}{}", "a", "b".repeat(32));
    assert_eq!(
        RelationKind::try_from(too_long.as_str()).unwrap_err(),
        RelationKindError::TooLong(33),
        "33 characters exceed the published 32-character limit"
    );
}

#[test]
fn grammar_boundaries_are_accepted_exactly() {
    // 32 characters: exactly at the published limit.
    let longest = format!("a{}", "b".repeat(31));
    RelationKind::try_from(longest.as_str())
        .expect("a 32-character token sits at the grammar limit and must parse");
}

#[test]
fn a_reply_names_its_parent_with_explicit_direction() {
    let relation = PostRelation {
        referencing_post_id: "child-post-id".to_owned(),
        kind: RelationKind::try_from("reply").unwrap(),
        target: RelationTarget::Resolved(ResolvedTarget {
            provider_post_id: "parent-post-id".to_owned(),
        }),
    };

    assert_eq!(
        relation.referencing_post_id, "child-post-id",
        "direction starts at the referencing post"
    );
    assert_eq!(
        relation.kind.as_str(),
        "reply",
        "the edge reports its documented kind"
    );
    assert!(
        matches!(relation.target, RelationTarget::Resolved(_)),
        "a reply whose parent is resolved keeps a resolved target"
    );
    let RelationTarget::Resolved(target) = &relation.target else {
        return; // unreachable: the matches! assertion above failed first
    };
    assert_eq!(
        target.provider_post_id, "parent-post-id",
        "the parent is named by its stable provider external id"
    );
}

#[test]
fn an_unavailable_parent_stays_an_unresolved_relation() {
    let evidence = UnresolvedTarget {
        provider_post_id: Some("parent-post-id".to_owned()),
        permalink: Some("https://www.threads.net/@example/post/123".to_owned()),
    };
    let relation = PostRelation {
        referencing_post_id: "child-post-id".to_owned(),
        kind: RelationKind::try_from("quote").unwrap(),
        target: RelationTarget::Unresolved(evidence.clone()),
    };

    assert!(
        matches!(relation.target, RelationTarget::Unresolved(_)),
        "an unresolved parent must stay unresolved, not invented"
    );
    let RelationTarget::Unresolved(unresolved) = &relation.target else {
        return; // unreachable: the matches! assertion above failed first
    };
    assert_eq!(
        unresolved, &evidence,
        "unresolved targets preserve exactly the held evidence"
    );
    assert_ne!(
        unresolved.provider_post_id,
        Some(relation.referencing_post_id.clone()),
        "target evidence is never confused with the referencing post"
    );
}
