//! The reply, quote, and repost edge contract: which relation kinds exist on
//! the wire, how direction and targets are named, and why an edge whose target
//! is unavailable is stored as unresolved instead of dropped.
//!
//! The relation-kind grammar is the published `SocialRelationKind` token of
//! `ratatoskr-social-contracts` (`crates/social-contracts/src/relation.rs`,
//! revision `fb88f94`): lowercase letters, digits, and underscores, starting
//! with a letter, at most 32 characters — open on purpose, so a provider edge
//! kind this service does not model yet is kept, never discarded.

use std::fmt;

/// The longest relation-kind token the published grammar accepts.
const MAX_LEN: usize = 32;

/// How a post references another post: `reply`, `quote`, `repost`, or another
/// provider edge kind.
///
/// **Open on purpose**, like the published grammar: an unknown but well-formed
/// kind is preserved as itself rather than refused or rewritten, so provider
/// structure survives even when this service does not model it yet. Parsing
/// enforces only the published token shape.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationKind(String);

impl RelationKind {
    /// The wire value of this relation kind.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for RelationKind {
    type Error = RelationKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::validate(value)?;
        Ok(RelationKind(value.to_owned()))
    }
}

impl RelationKind {
    /// Check a token against the published grammar before it becomes one.
    ///
    /// The first violated rule wins, so the error names what the caller must fix.
    fn validate(token: &str) -> Result<(), RelationKindError> {
        let mut chars = token.chars();
        match chars.next() {
            None => return Err(RelationKindError::Empty),
            Some(first) if !first.is_ascii_lowercase() => {
                return Err(RelationKindError::LeadingCharacter(first));
            }
            Some(_) => {}
        }
        if let Some(illegal) = chars.find(|character| {
            !(character.is_ascii_lowercase() || character.is_ascii_digit() || *character == '_')
        }) {
            return Err(RelationKindError::IllegalCharacter(illegal));
        }
        if token.chars().count() > MAX_LEN {
            return Err(RelationKindError::TooLong(token.chars().count()));
        }
        Ok(())
    }
}

impl TryFrom<String> for RelationKind {
    type Error = RelationKindError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(RelationKind(value))
    }
}

/// Why a relation-kind token was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationKindError {
    /// The token was empty.
    #[error("relation kind must not be empty")]
    Empty,
    /// The token did not start with a lowercase ASCII letter.
    #[error("relation kind must start with a lowercase ASCII letter, found {0:?}")]
    LeadingCharacter(char),
    /// The token contained a character outside `[a-z0-9_]`.
    #[error(
        "relation kind may only contain lowercase ASCII letters, digits, and underscores, found {0:?}"
    )]
    IllegalCharacter(char),
    /// The token exceeded the grammar's 32-character limit.
    #[error("relation kind exceeds the published 32-character limit (length {0})")]
    TooLong(usize),
}

/// A directed edge from one post to the post it references.
///
/// Direction is explicit: `referencing_post_id` names the reply, quote, or
/// repost; `target` names the post it points at. Targets are always named by
/// stable provider external id, mirroring the published `SocialRelation`
/// shape; relations never cross platforms, so no platform field is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRelation {
    /// The referencing post — the reply, quote, or repost — named by its
    /// stable provider external id.
    pub referencing_post_id: String,
    /// What kind of edge this is.
    pub kind: RelationKind,
    /// Where the edge points.
    pub target: RelationTarget,
}

/// The end of a relation edge, resolved or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationTarget {
    /// The referenced post exists as a local source record; still named by its
    /// stable provider external id so consumers can join by platform identity.
    Resolved(ResolvedTarget),
    /// No local source record exists for the target yet; whatever evidence the
    /// referencing post carried is preserved and nothing is invented.
    Unresolved(UnresolvedTarget),
}

/// A relation target that exists as a local source record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// The target post's stable provider external id.
    pub provider_post_id: String,
}

/// Evidence held about a relation target that has not been resolved into a
/// local source record.
///
/// An unavailable parent does not invalidate the captured child: the relation
/// stays stored with exactly the evidence below, and no target content is
/// synthesized.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnresolvedTarget {
    /// Stable provider external id when the referencing post exposed one.
    pub provider_post_id: Option<String>,
    /// Canonical permalink when one was observed.
    pub permalink: Option<String>,
}
