//! App-version comparison, matching npm-semver and the TypeScript front-end.
//!
//! `is_update_available` coerces each side like npm-semver
//! `coerce(_, { includePrerelease: true })` - the leftmost `X[.Y[.Z]]` numeric
//! triple (tolerating a leading `v` and surrounding text) plus an immediately
//! following `-<tag>` pre-release - and compares by full SemVer precedence. Both
//! the app-update check and the `minWindhawkVersion` gate use it, so a
//! pre-release orders below its release and below later pre-releases
//! (`2.0.0-alpha.1` < `2.0.0-alpha.2` < `2.0.0-beta.1` < `2.0.0`): an alpha is
//! told about its final release, and does NOT satisfy a `minWindhawkVersion` of
//! its own release. Either side failing to coerce makes the comparison
//! unavailable and the caller treats it as "no update"/"gate passes".
//!
//! `coerce` is the tag-dropping triple used only to pack the compiler's
//! `WH_WINDHAWK_VERSION` define (services::compiler), not for comparison.
//!
//! Mod versions use a different rule (plain string inequality) and do not go
//! through here; that lives at each mod-version call site.

use std::cmp::Ordering;

/// `semver.coerce`: the leftmost numeric `major[.minor[.patch]]`, with missing
/// components defaulting to 0. `None` when the string has no numeric component
/// (or a component overflows `u64`, which no real version does).
pub fn coerce(s: &str) -> Option<(u64, u64, u64)> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(u8::is_ascii_digit)?;

    // Read a maximal run of ASCII digits starting at `i`, advancing it.
    fn read_num(bytes: &[u8], i: &mut usize) -> Option<u64> {
        let begin = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == begin {
            return None;
        }
        std::str::from_utf8(&bytes[begin..*i])
            .ok()?
            .parse::<u64>()
            .ok()
    }

    let mut i = start;
    let major = read_num(bytes, &mut i)?;
    let mut minor = 0;
    let mut patch = 0;

    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        if let Some(m) = read_num(bytes, &mut j) {
            minor = m;
            i = j;
            if i < bytes.len() && bytes[i] == b'.' {
                let mut k = i + 1;
                if let Some(p) = read_num(bytes, &mut k) {
                    patch = p;
                }
            }
        }
    }

    Some((major, minor, patch))
}

/// A SemVer pre-release identifier. Numeric identifiers order below alphanumeric
/// ones and compare numerically.
#[derive(PartialEq, Eq)]
enum PrereleaseId {
    Numeric(u64),
    Alphanumeric(String),
}

impl Ord for PrereleaseId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (PrereleaseId::Numeric(a), PrereleaseId::Numeric(b)) => a.cmp(b),
            (PrereleaseId::Numeric(_), PrereleaseId::Alphanumeric(_)) => Ordering::Less,
            (PrereleaseId::Alphanumeric(_), PrereleaseId::Numeric(_)) => Ordering::Greater,
            (PrereleaseId::Alphanumeric(a), PrereleaseId::Alphanumeric(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PrereleaseId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A version parsed for full SemVer precedence: the numeric release triple plus
/// the pre-release identifiers (empty for a final release).
#[derive(PartialEq, Eq)]
struct PrereleaseVersion {
    release: (u64, u64, u64),
    prerelease: Vec<PrereleaseId>,
}

impl Ord for PrereleaseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.release.cmp(&other.release).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                // A final release outranks any pre-release of the same triple.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                // Vec ordering compares identifier-by-identifier and makes the
                // shorter set the lesser when the shared prefix is equal, which
                // is the SemVer "larger set has higher precedence" rule.
                (false, false) => self.prerelease.cmp(&other.prerelease),
            }
        })
    }
}

impl PartialOrd for PrereleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `coerce(s, { includePrerelease: true })`: the leftmost numeric
/// `major[.minor[.patch]]` (missing components default to 0) plus an
/// immediately following `-<tag>` pre-release, ending at a `+` (build metadata,
/// ignored) or whitespace. `None` when the string has no numeric component.
fn coerce_with_prerelease(s: &str) -> Option<PrereleaseVersion> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(u8::is_ascii_digit)?;

    fn read_num(bytes: &[u8], i: &mut usize) -> Option<u64> {
        let begin = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == begin {
            return None;
        }
        std::str::from_utf8(&bytes[begin..*i])
            .ok()?
            .parse::<u64>()
            .ok()
    }

    let mut i = start;
    let major = read_num(bytes, &mut i)?;
    let mut minor = 0;
    let mut patch = 0;

    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        if let Some(m) = read_num(bytes, &mut j) {
            minor = m;
            i = j;
            if i < bytes.len() && bytes[i] == b'.' {
                let mut k = i + 1;
                if let Some(p) = read_num(bytes, &mut k) {
                    patch = p;
                    i = k;
                }
            }
        }
    }

    let mut prerelease = Vec::new();
    if i < bytes.len() && bytes[i] == b'-' {
        let begin = i + 1;
        let mut end = begin;
        while end < bytes.len() && bytes[end] != b'+' && !bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        for id in s[begin..end].split('.') {
            match id.parse::<u64>() {
                Ok(n) if !id.is_empty() => prerelease.push(PrereleaseId::Numeric(n)),
                _ => prerelease.push(PrereleaseId::Alphanumeric(id.to_owned())),
            }
        }
    }

    Some(PrereleaseVersion {
        release: (major, minor, patch),
        prerelease,
    })
}

/// True only when both sides coerce (keeping the pre-release tag) and `current`
/// precedes `candidate` by full SemVer precedence. Used by the app-update check
/// and the `minWindhawkVersion` gate. An uncoercible side makes it false (the
/// "no update" / "gate passes" case).
pub fn is_update_available(current: Option<&str>, candidate: Option<&str>) -> bool {
    match (
        current.and_then(coerce_with_prerelease),
        candidate.and_then(coerce_with_prerelease),
    ) {
        (Some(current), Some(candidate)) => current < candidate,
        _ => false,
    }
}

/// True when `version` coerces to a SemVer carrying a pre-release tag
/// (e.g. `2.0.0-alpha.1`); an uncoercible or final-release version is not a
/// pre-release. Used to decide whether a running build is on the pre-release
/// channel and should fold the cached pre-release version into its update check.
pub fn is_pre_release(version: &str) -> bool {
    coerce_with_prerelease(version).is_some_and(|v| !v.prerelease.is_empty())
}

/// The higher of a cached "latest" and an `extra` candidate, by the same full
/// SemVer precedence [`is_update_available`] uses: returns `base` only when
/// `extra` strictly precedes it, so folding `extra` in never lowers the offered
/// version. An absent or uncoercible `base` yields `extra`.
pub fn higher_version<'a>(base: Option<&'a str>, extra: &'a str) -> &'a str {
    match base {
        Some(base) if is_update_available(Some(extra), Some(base)) => base,
        _ => extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_extracts_the_first_triple() {
        assert_eq!(coerce("1.6.1"), Some((1, 6, 1)));
        assert_eq!(coerce("v1.8.0-beta"), Some((1, 8, 0)));
        assert_eq!(coerce("1.2"), Some((1, 2, 0)));
        assert_eq!(coerce("Windhawk 1.7"), Some((1, 7, 0)));
        assert_eq!(coerce("7"), Some((7, 0, 0)));
        assert_eq!(coerce("none"), None);
        assert_eq!(coerce(""), None);
    }

    #[test]
    fn update_availability() {
        assert!(is_update_available(Some("1.6.1"), Some("1.8.0")));
        assert!(!is_update_available(Some("1.8.0"), Some("1.8.0")));
        assert!(!is_update_available(Some("1.9.0"), Some("1.8.0")));
        // Uncoercible installed version -> no update (the unknown-version case).
        assert!(!is_update_available(None, Some("1.8.0")));
        assert!(!is_update_available(Some("dev"), Some("1.8.0")));
    }

    #[test]
    fn update_availability_orders_prereleases() {
        // A pre-release precedes its final release and later pre-releases: the
        // update check offers them, and the minWindhawkVersion gate blocks a mod
        // that requires a version this pre-release precedes.
        assert!(is_update_available(Some("2.0.0-alpha.1"), Some("2.0.0")));
        assert!(is_update_available(
            Some("2.0.0-alpha.1"),
            Some("2.0.0-alpha.2")
        ));
        assert!(is_update_available(
            Some("2.0.0-alpha.2"),
            Some("2.0.0-alpha.10")
        ));
        assert!(is_update_available(
            Some("2.0.0-alpha.9"),
            Some("2.0.0-beta.1")
        ));
        assert!(is_update_available(Some("1.7.3"), Some("2.0.0-alpha.1")));

        // Not less: down to an older/own pre-release, or equal to itself.
        assert!(!is_update_available(Some("2.0.0"), Some("2.0.0-alpha.1")));
        assert!(!is_update_available(Some("2.0.0-alpha.1"), Some("1.7.3")));
        assert!(!is_update_available(
            Some("2.0.0-alpha.1"),
            Some("2.0.0-alpha.1")
        ));
        assert!(!is_update_available(Some("2.0.0"), Some("2.0.0")));

        // A trimmed base ("2.0-alpha.1", emitted by the C++ app) and the full
        // form compare equal.
        assert!(!is_update_available(
            Some("2.0-alpha.1"),
            Some("2.0.0-alpha.1")
        ));

        // Uncoercible side -> no update.
        assert!(!is_update_available(None, Some("2.0.0")));
        assert!(!is_update_available(Some("dev"), Some("2.0.0")));
    }

    #[test]
    fn pre_release_detection() {
        assert!(is_pre_release("2.0.0-alpha.1"));
        assert!(is_pre_release("2.0-beta.1")); // trimmed base, C++ form
        assert!(is_pre_release("v1.8.0-rc.2"));
        assert!(!is_pre_release("1.7.3"));
        assert!(!is_pre_release("2.0.0"));
        assert!(!is_pre_release("dev")); // uncoercible is not a pre-release
        assert!(!is_pre_release(""));
    }

    #[test]
    fn higher_version_folds_in_the_greater() {
        // extra outranks base -> extra; base outranks extra -> base; tie -> extra.
        assert_eq!(
            higher_version(Some("1.7.3"), "2.0.0-alpha.2"),
            "2.0.0-alpha.2"
        );
        assert_eq!(higher_version(Some("2.5.0"), "2.0.0-alpha.2"), "2.5.0");
        assert_eq!(higher_version(Some("2.0.0"), "2.0.0"), "2.0.0");
        // Absent base -> extra.
        assert_eq!(higher_version(None, "2.0.0-alpha.1"), "2.0.0-alpha.1");
        // Uncoercible base -> the coercible extra wins (never offer garbage).
        assert_eq!(
            higher_version(Some("dev"), "2.0.0-alpha.1"),
            "2.0.0-alpha.1"
        );
    }
}
