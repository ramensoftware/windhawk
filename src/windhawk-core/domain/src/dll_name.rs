//! The compiled-DLL name vocabulary: the `<id>_<ver>_<6 digits>.dll` format
//! producer, the `ends_with_random_suffix` recognizer, and the LCG that
//! generates the 6-digit "random" suffix.
//!
//! The LCG is a SEED step plus a PER-ITERATION step, NOT one `seed_ms -> u64`
//! function: `random_six` (the download side) seeds then takes ONE step;
//! `unique_dll_name` (the compile side) seeds ONCE then steps PER iteration of a
//! `files.exists` collision loop, so each pass yields a DIFFERENT suffix. A
//! single seed-and-step function would return the same value every call and loop
//! forever. The collision loop itself stays in `compiler` (it touches the
//! `Files` port, which must not enter this pure leaf crate).
//!
//! The LCG arithmetic constants below are ARBITRARY: only the 6-digit suffix
//! RANGE and the `<id>_<ver>_<digits>.dll` FORMAT are a contract (recognized by
//! `ends_with_random_suffix` and the catalog parity). Uniqueness in production
//! comes from the compile-side collision loop, not from the generator's
//! quality, so any deterministic-under-test generator would do; do not mistake
//! these for tuned or TS-derived values (the TS uses `Math.random()`).

/// XOR mixer applied once to the clock-derived seed (the golden-ratio constant).
const LCG_SEED_MIXER: u64 = 0x9E37_79B9_7F4A_7C15;
/// LCG multiplier (the common SplitMix/PCG step constant).
const LCG_MULTIPLIER: u64 = 6364136223846793005;
/// LCG increment (odd, so the period is full).
const LCG_INCREMENT: u64 = 1442695040888963407;
/// The inclusive suffix range is `[SUFFIX_MIN, SUFFIX_MIN + SUFFIX_SPAN - 1]` =
/// `[100000, 999999]` (the TS `randomIntFromInterval(100000, 999999)`).
const SUFFIX_MIN: u64 = 100_000;
const SUFFIX_SPAN: u64 = 900_000;

/// Seed the LCG from the clock-derived ms (the `seed_ms ^ golden` XOR-once
/// step). Advance the returned state with `lcg_next_six`.
pub fn lcg_seed(seed_ms: i64) -> u64 {
    (seed_ms as u64) ^ LCG_SEED_MIXER
}

/// One LCG step over `state` (multiply-add), folding the high bits into a
/// 6-digit number (100000..=999999). Each call advances `state`, so a
/// collision loop calling it repeatedly gets a fresh suffix per iteration.
pub fn lcg_next_six(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(LCG_MULTIPLIER)
        .wrapping_add(LCG_INCREMENT);
    SUFFIX_MIN + (*state >> 33) % SUFFIX_SPAN
}

/// The compiled-DLL name `<mod_id>_<version>_<6 digits>.dll`.
pub fn compiled_dll_name(mod_id: &str, version: &str, rand6: u64) -> String {
    format!("{mod_id}_{version}_{rand6}.dll")
}

/// The TS `(^|_)[0-9]+$` test on the filename part between `<modId>_` and
/// `.dll`: a trailing run of digits, preceded by the start or an underscore.
pub fn ends_with_random_suffix(part: &str) -> bool {
    let bytes = part.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    // Needs at least one trailing digit, preceded by start-of-string or '_'.
    i < bytes.len() && (i == 0 || bytes[i - 1] == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_suffix_matches_the_ts_regex() {
        assert!(ends_with_random_suffix("1.0_654321"));
        assert!(ends_with_random_suffix("654321"));
        assert!(ends_with_random_suffix("1_2_3"));
        assert!(!ends_with_random_suffix("1.0"));
        assert!(!ends_with_random_suffix("beta"));
        assert!(!ends_with_random_suffix(""));
    }

    #[test]
    fn next_six_is_six_digits_and_advances_the_state() {
        let mut state = lcg_seed(1_700_000_000_000);
        let a = lcg_next_six(&mut state);
        let b = lcg_next_six(&mut state);
        assert!((100_000..=999_999).contains(&a));
        assert!((100_000..=999_999).contains(&b));
        // Stepping again over the mutated state yields a different suffix (the
        // collision loop's per-iteration freshness).
        assert_ne!(a, b);
    }

    #[test]
    fn compiled_dll_name_formats_the_parts() {
        assert_eq!(
            compiled_dll_name("test-mod", "1.0", 654321),
            "test-mod_1.0_654321.dll"
        );
    }
}
