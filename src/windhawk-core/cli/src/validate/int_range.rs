//! int32 setting validation (`intRange.ts`).
//!
//! Numeric settings are stored as 32-bit values (REG_DWORD in non-portable
//! mode) and read by the engine as a 32-bit int. The signed 32-bit range is the
//! only one that round-trips correctly under both interpretations (unsigned
//! `[2^31, 2^32-1]` values would read back as negative ints), so values outside
//! it are rejected rather than silently truncated on write.

use crate::error::CliError;

pub const INT32_MIN: i64 = -2_147_483_648;
pub const INT32_MAX: i64 = 2_147_483_647;

/// Parse a setting string into a validated 32-bit integer. Rejects floats,
/// non-numeric input, and out-of-range values with a usage error (exit 2),
/// mirroring the TS `parseInt32Setting`.
pub fn parse_int32_setting(key: &str, raw: &str) -> Result<i64, CliError> {
    if !is_integer_literal(raw) {
        return Err(CliError::usage(format!(
            "Setting '{key}' must be an integer, got '{raw}'."
        )));
    }
    // A literal too large for i64 is necessarily outside the 32-bit range; treat
    // the parse failure as a range violation, not a "not an integer" one (the TS
    // `Number(raw)` produces an out-of-range float for the same input).
    match raw.parse::<i64>() {
        Ok(n) if (INT32_MIN..=INT32_MAX).contains(&n) => Ok(n),
        _ => Err(CliError::usage(format!(
            "Setting '{key}' must be a 32-bit integer ({INT32_MIN}..{INT32_MAX}), got '{raw}'."
        ))),
    }
}

/// `^-?\d+$`: an optional leading minus then one or more ASCII digits.
fn is_integer_literal(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_in_range_integers() {
        assert_eq!(parse_int32_setting("k", "0").unwrap(), 0);
        assert_eq!(parse_int32_setting("k", "-1").unwrap(), -1);
        assert_eq!(parse_int32_setting("k", "2147483647").unwrap(), INT32_MAX);
        assert_eq!(parse_int32_setting("k", "-2147483648").unwrap(), INT32_MIN);
        // Leading zeros parse like the TS `Number("007")`.
        assert_eq!(parse_int32_setting("k", "007").unwrap(), 7);
    }

    #[test]
    fn rejects_non_integers() {
        for raw in ["", "-", "1.5", "abc", "1e3", "0x10", " 1", "1 ", "+1"] {
            let err = parse_int32_setting("k", raw).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{raw}");
            assert!(err.message().contains("must be an integer"), "{raw}");
        }
    }

    #[test]
    fn rejects_out_of_range() {
        for raw in ["2147483648", "-2147483649", "99999999999999999999999"] {
            let err = parse_int32_setting("k", raw).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{raw}");
            assert!(err.message().contains("32-bit integer"), "{raw}");
        }
    }
}
