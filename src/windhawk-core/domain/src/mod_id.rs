//! The `ModId` and `Version` newtypes: thin wrappers over `String` that make a
//! mod id and a mod version TYPE-DISTINCT where they travel together at the
//! swap-prone compile/install surface (adjacent same-typed `&str` params were
//! swappable with no compile error). The `local@` prefix predicate lives here,
//! with its literal in ONE home.
//!
//! Scope: the wire DTOs in `protocol` stay `String` (a leaf crate that may not
//! depend on `domain`); the service wraps at the compile/install entry and
//! unwraps (`as_str()`) at the inherently-stringly boundaries (storage tree
//! keys, repo URLs, profile map keys, the keyed mod lock). Only `as_str()` /
//! `Display` / `From` / `AsRef<str>` are exposed - deliberately NOT
//! `Deref<Target = str>`, which would erase the explicit unwrap and re-leak
//! `str`'s whole surface, eroding the type-distinctness this exists to create.

use std::fmt;

/// A mod's storage id (`<id>` or `local@<id>`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModId(String);

impl ModId {
    /// The prefix marking a locally-authored mod. Its ONE home (folds the
    /// open-coded `starts_with("local@")` sites onto the predicates below).
    const LOCAL_PREFIX: &'static str = "local@";

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this id is a locally-authored mod (`local@<id>`).
    pub fn is_local(&self) -> bool {
        Self::str_is_local(&self.0)
    }

    /// The borrowing predicate, for the many sites that hold a bare `&str` /
    /// `&String` (union-loop variables, a `storage_id: &str`) and must not
    /// clone just to test the prefix.
    pub fn str_is_local(s: &str) -> bool {
        s.starts_with(Self::LOCAL_PREFIX)
    }

    /// A storage id with the `local@` prefix stripped: the bare id a mod's
    /// source declares as its `@id`. A bare id is returned unchanged.
    pub fn str_bare(s: &str) -> &str {
        s.strip_prefix(Self::LOCAL_PREFIX).unwrap_or(s)
    }

    /// Whether `s` is a well-formed BARE id: non-empty and drawn only from
    /// `0-9`, `a-z`, and `-`. That charset is what keeps an id safe to use
    /// verbatim as a path component and a registry key name, so it is enforced
    /// on every id that reaches storage, not only on the one a source declares.
    pub fn str_is_valid_bare(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='z' | '-'))
    }
}

impl From<String> for ModId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ModId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for ModId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A mod version string (`@version`). Taken alongside `ModId` so
/// `(&ModId, &Version)` reads as intent at the swap-prone surface; carries no
/// behavior beyond `as_str`/`Display`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version(String);

impl Version {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `s` is a well-formed version string: non-empty and drawn only
    /// from `0-9`, `a-z`, `A-Z`, `.`, `-`, `_`, and `+`. The charset excludes
    /// every character that could restructure a URL path (`/`, `\`, `%`, `?`,
    /// `#`, whitespace), which is what keeps a version safe to interpolate
    /// verbatim into a repository URL.
    pub fn str_is_valid(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| matches!(c, '0'..='9' | 'a'..='z' | 'A'..='Z' | '.' | '-' | '_' | '+'))
    }
}

impl From<String> for Version {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Version {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for Version {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_local_detects_the_prefix_both_ways() {
        assert!(ModId::from("local@my-mod").is_local());
        assert!(!ModId::from("my-mod").is_local());
        assert!(ModId::str_is_local("local@x"));
        assert!(!ModId::str_is_local("x"));
    }

    #[test]
    fn version_charset_excludes_url_structure() {
        for v in ["1.0", "1.2.3", "2024.01", "1.0.0-beta.1+build_5"] {
            assert!(Version::str_is_valid(v), "{v:?} must be valid");
        }
        for v in [
            "", "../evil", "..\\evil", "%2e%2e", "?q", "#f", "a b", "a\r\n", "a:b", "a&b",
        ] {
            assert!(!Version::str_is_valid(v), "{v:?} must be rejected");
        }
    }

    #[test]
    fn as_str_and_display_round_trip() {
        let id = ModId::from("m".to_owned());
        assert_eq!(id.as_str(), "m");
        assert_eq!(id.to_string(), "m");
        let v = Version::from("1.0");
        assert_eq!(v.as_str(), "1.0");
        assert_eq!(v.to_string(), "1.0");
    }
}
