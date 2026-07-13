//! Pure value encodings shared by the settings services
//! (`services::app_settings`, `services::mods`, `services::install`) and the
//! `SettingsTree`-bound read/write helpers in `services::settings_io`: the
//! on-disk type tags of the front-end's field descriptors. Pipe-joined string
//! arrays (`split_pipe`/`join_pipe`) AND the boolean<->int conversion
//! (`bool_to_int`/`int_to_bool`) live here, in ONE home, so a write site and
//! the read it inverts cannot drift. DECODE is the LENIENT `i != 0` (any
//! nonzero is `true`), NOT the strict inverse `i == 1` - see `int_to_bool`.

/// `splitPipeDelimited`: an empty string is the empty list (not `[""]`); any
/// other string splits on `|`.
pub fn split_pipe(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split('|').map(str::to_owned).collect()
    }
}

/// The inverse: `value.join('|')`. The empty list is the empty string.
pub fn join_pipe(values: &[String]) -> String {
    values.join("|")
}

/// Encode a boolean as the stored 0/1 int (the front-end descriptor stores
/// booleans as a `REG_DWORD` / decimal).
pub fn bool_to_int(value: bool) -> i32 {
    if value { 1 } else { 0 }
}

/// Decode a stored int back to a boolean. LENIENT: any nonzero reads as `true`
/// (mirroring the TS `!!value` and the historical `i != 0`), NOT the strict
/// inverse `i == 1` of `bool_to_int`. DO NOT "tidy" this to `i == 1`: a stored
/// `2` or `-1` must read as `true`, not be silently dropped to `false`.
pub fn int_to_bool(value: i32) -> bool {
    value != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_join_round_trip() {
        assert_eq!(split_pipe(""), Vec::<String>::new());
        assert_eq!(split_pipe("a"), vec!["a"]);
        assert_eq!(split_pipe("a|b|c"), vec!["a", "b", "c"]);
        assert_eq!(join_pipe(&[]), "");
        assert_eq!(join_pipe(&["a".into(), "b".into()]), "a|b");
        // A single empty entry survives a round trip only as the empty list.
        assert_eq!(split_pipe(&join_pipe(&[])), Vec::<String>::new());
    }

    #[test]
    fn bool_codec_encodes_0_1_and_decodes_leniently() {
        assert_eq!(bool_to_int(false), 0);
        assert_eq!(bool_to_int(true), 1);
        assert!(!int_to_bool(0));
        assert!(int_to_bool(1));
        // Lenient decode: any nonzero is true (the TS `!!value`), not just 1.
        assert!(int_to_bool(2));
        assert!(int_to_bool(-1));
    }
}
