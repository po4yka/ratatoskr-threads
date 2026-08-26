//! Canonicalization of Threads post permalinks for the explicit-capture lane.
//!
//! The accepted input grammar is exactly `<scheme>://<host>/@<handle>/post/<code>`
//! where the scheme is `http` or `https`, the host is one of `threads.net`,
//! `www.threads.net`, `threads.com`, or `www.threads.com` with an optional
//! default port (`80` or `443`), the handle is 1..=30 characters of ASCII
//! letters, digits, periods, and underscores, and the post code is 1..=128
//! characters of ASCII letters, digits, underscores, and hyphens. A tracking
//! query string, a fragment, and trailing slashes after the code are stripped;
//! nothing else is repaired.
//!
//! Every accepted form produces one canonical value,
//! `https://www.threads.net/@<lowercased-handle>/post/<code>`: the scheme is
//! upgraded to `https`, the host normalized to `www.threads.net`, the default
//! port dropped, and the handle case folded. Post codes are case-sensitive
//! provider tokens and stay verbatim. The canonicalization result carries the
//! original input text byte-for-byte next to the canonical permalink.
//!
//! The `/t/<code>` short form is refused at intake because resolving it
//! requires the public-resolution lane; nothing here performs network I/O.
//!
//! The entry point is [`std::convert::TryFrom<&str>`] for
//! [`CanonicalizedUrl`]; every input outside the documented grammar is refused
//! with a [`PermalinkError`] naming the violated rule. The first violated rule
//! wins, so callers can fix inputs one defect at a time.

/// The canonical host every accepted input normalizes to.
const CANONICAL_HOST: &str = "www.threads.net";

/// The provider hosts accepted as permalink authorities, compared ASCII
/// case-insensitively; anything else, including lookalike subdomains and
/// suffixes, is refused.
const ACCEPTED_HOSTS: [&str; 4] = [
    "threads.net",
    "www.threads.net",
    "threads.com",
    "www.threads.com",
];

/// The longest raw URL text canonicalization accepts, counted before any
/// parsing so hostile input stays bounded.
const MAX_INPUT_LEN: usize = 2048;

/// The longest handle the provider handle grammar accepts.
const HANDLE_MAX_LEN: usize = 30;

/// The longest post code the post-code grammar accepts.
const CODE_MAX_LEN: usize = 128;

fn is_handle_byte(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'_')
}

fn is_code_byte(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-')
}

/// A canonical Threads post permalink:
/// `https://www.threads.net/@<handle>/post/<code>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Permalink(String);

impl Permalink {
    /// The canonical permalink text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The outcome of canonicalizing one raw URL text: the byte-for-byte original
/// input next to the canonical permalink derived from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalizedUrl {
    original: String,
    permalink: Permalink,
}

impl CanonicalizedUrl {
    /// The original input text, unchanged.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// The canonical permalink.
    #[must_use]
    pub fn permalink(&self) -> &Permalink {
        &self.permalink
    }
}

impl TryFrom<&str> for CanonicalizedUrl {
    type Error = PermalinkError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let raw = value;
        let length = raw.chars().count();
        if length > MAX_INPUT_LEN {
            return Err(PermalinkError::TooLong(length));
        }
        if !raw.is_ascii() {
            return Err(PermalinkError::NonAscii);
        }
        if raw.contains('%') {
            return Err(PermalinkError::PercentEncoded);
        }

        let Some((scheme, rest)) = raw.split_once("://") else {
            return Err(PermalinkError::InvalidUrl);
        };
        if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
            return Err(PermalinkError::Scheme);
        }

        let without_fragment = if let Some((before, _fragment)) = rest.split_once('#') {
            before
        } else {
            rest
        };
        let without_query = if let Some((before, _query)) = without_fragment.split_once('?') {
            before
        } else {
            without_fragment
        };

        let (authority, path) = if let Some((host_part, path_part)) = without_query.split_once('/')
        {
            (host_part, path_part)
        } else {
            (without_query, "")
        };

        let host = if let Some((bare_host, port)) = authority.rsplit_once(':') {
            if port != "80" && port != "443" {
                return Err(PermalinkError::Port);
            }
            bare_host
        } else {
            authority
        };
        if !ACCEPTED_HOSTS
            .iter()
            .any(|accepted| host.eq_ignore_ascii_case(accepted))
        {
            return Err(PermalinkError::Host);
        }

        if path.starts_with("t/") {
            return Err(PermalinkError::ShortFormUnsupported);
        }
        let Some(after_marker) = path.strip_prefix('@') else {
            return Err(PermalinkError::PathShape);
        };
        let Some((handle, code_with_trailing_slashes)) = after_marker.split_once("/post/") else {
            return Err(PermalinkError::PathShape);
        };
        let code = code_with_trailing_slashes.trim_end_matches('/');

        let handle_valid = !handle.is_empty()
            && handle.len() <= HANDLE_MAX_LEN
            && handle.bytes().all(is_handle_byte);
        if !handle_valid {
            return Err(PermalinkError::HandleGrammar);
        }
        let code_valid =
            !code.is_empty() && code.len() <= CODE_MAX_LEN && code.bytes().all(is_code_byte);
        if !code_valid {
            return Err(PermalinkError::PostCodeGrammar);
        }

        let canonical = format!(
            "https://{CANONICAL_HOST}/@{}/post/{code}",
            handle.to_ascii_lowercase(),
        );
        Ok(CanonicalizedUrl {
            original: raw.to_owned(),
            permalink: Permalink(canonical),
        })
    }
}

/// Why a raw URL text was refused at intake.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PermalinkError {
    /// The input was not an absolute URL carrying an explicit scheme.
    #[error("permalink must be an absolute http or https URL")]
    InvalidUrl,
    /// The scheme did not name `http` or `https`.
    #[error("permalink scheme must be http or https")]
    Scheme,
    /// The host was not one of the four documented provider hosts.
    #[error("permalink host must be threads.net, www.threads.net, threads.com, or www.threads.com")]
    Host,
    /// A port was present and it was not a scheme-default port (`80`/`443`).
    #[error("permalink may carry no port other than the scheme defaults 80 and 443")]
    Port,
    /// The path did not have the `/@<handle>/post/<code>` shape.
    #[error("permalink path must have the /@handle/post/code shape")]
    PathShape,
    /// The handle violated its grammar inside the post-path shape.
    #[error(
        "the /@handle/post/code path shape requires a handle of 1..=30 ASCII letters, digits, \
         periods, or underscores"
    )]
    HandleGrammar,
    /// The post code violated its grammar inside the post-path shape.
    #[error(
        "the /@handle/post/code path shape requires a post code of 1..=128 ASCII letters, digits, \
         underscores, or hyphens"
    )]
    PostCodeGrammar,
    /// The input contained percent-escapes instead of decoded text.
    #[error("permalink must not contain percent-escapes; submit the decoded URL text")]
    PercentEncoded,
    /// The input contained non-ASCII characters.
    #[error("permalink must be ASCII-only")]
    NonAscii,
    /// The raw input exceeded the intake length cap.
    #[error("permalink exceeds the 2048-character intake limit (length {0})")]
    TooLong(usize),
    /// The `/t/<code>` short form cannot be canonicalized textually.
    #[error(
        "/t/<code> short form is unsupported at intake; resolving it requires the \
            public-resolution lane"
    )]
    ShortFormUnsupported,
}
