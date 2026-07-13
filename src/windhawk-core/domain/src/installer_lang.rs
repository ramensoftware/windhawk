//! Mapping a Windhawk UI language code to the installer's language LCID
//! (`applyAppSettings`, non-portable mode), reproducing
//! `setInstallerLanguage` in the front-end's `services/appSettings.ts`.
//!
//! The match is a prefix match over the LCID table below, in table order:
//! the input's `-`-separated parts must be a prefix of a candidate's
//! `_`-separated parts, and the first such candidate wins. The single
//! special case (`es` / `es-ES` -> 1034 is remapped to 3082, "Spanish
//! International") is applied after the match. An unmatched language yields
//! `None`, and the caller writes nothing - exactly the TS behavior.

/// The LCID table in the exact order of `setInstallerLanguage`'s `lcidMap`
/// (order is significant: the first prefix match wins). Sourced from
/// sindresorhus/lcid, as the TS comment records.
const LCID_LANGUAGES: &[(u32, &str)] = &[
    (4, "zh_CHS"),
    (1025, "ar_SA"),
    (1026, "bg_BG"),
    (1027, "ca_ES"),
    (1028, "zh_TW"),
    (1029, "cs_CZ"),
    (1030, "da_DK"),
    (1031, "de_DE"),
    (1032, "el_GR"),
    (1033, "en_US"),
    (1034, "es_ES"),
    (1035, "fi_FI"),
    (1036, "fr_FR"),
    (1037, "he_IL"),
    (1038, "hu_HU"),
    (1039, "is_IS"),
    (1040, "it_IT"),
    (1041, "ja_JP"),
    (1042, "ko_KR"),
    (1043, "nl_NL"),
    (1044, "nb_NO"),
    (1045, "pl_PL"),
    (1046, "pt_BR"),
    (1047, "rm_CH"),
    (1048, "ro_RO"),
    (1049, "ru_RU"),
    (1050, "hr_HR"),
    (1051, "sk_SK"),
    (1052, "sq_AL"),
    (1053, "sv_SE"),
    (1054, "th_TH"),
    (1055, "tr_TR"),
    (1056, "ur_PK"),
    (1057, "id_ID"),
    (1058, "uk_UA"),
    (1059, "be_BY"),
    (1060, "sl_SI"),
    (1061, "et_EE"),
    (1062, "lv_LV"),
    (1063, "lt_LT"),
    (1064, "tg_TJ"),
    (1065, "fa_IR"),
    (1066, "vi_VN"),
    (1067, "hy_AM"),
    (1069, "eu_ES"),
    (1070, "wen_DE"),
    (1071, "mk_MK"),
    (1072, "st_ZA"),
    (1073, "ts_ZA"),
    (1074, "tn_ZA"),
    (1075, "ven_ZA"),
    (1076, "xh_ZA"),
    (1077, "zu_ZA"),
    (1078, "af_ZA"),
    (1079, "ka_GE"),
    (1080, "fo_FO"),
    (1081, "hi_IN"),
    (1082, "mt_MT"),
    (1083, "se_NO"),
    (1084, "gd_GB"),
    (1085, "yi"),
    (1086, "ms_MY"),
    (1087, "kk_KZ"),
    (1088, "ky_KG"),
    (1089, "sw_KE"),
    (1090, "tk_TM"),
    (1092, "tt_RU"),
    (1093, "bn_IN"),
    (1094, "pa_IN"),
    (1095, "gu_IN"),
    (1096, "or_IN"),
    (1097, "ta_IN"),
    (1098, "te_IN"),
    (1099, "kn_IN"),
    (1100, "ml_IN"),
    (1101, "as_IN"),
    (1102, "mr_IN"),
    (1103, "sa_IN"),
    (1104, "mn_MN"),
    (1105, "bo_CN"),
    (1106, "cy_GB"),
    (1107, "kh_KH"),
    (1108, "lo_LA"),
    (1109, "my_MM"),
    (1110, "gl_ES"),
    (1111, "kok_IN"),
    (1113, "sd_IN"),
    (1114, "syr_SY"),
    (1115, "si_LK"),
    (1116, "chr_US"),
    (1118, "am_ET"),
    (1119, "tmz"),
    (1121, "ne_NP"),
    (1122, "fy_NL"),
    (1123, "ps_AF"),
    (1124, "fil_PH"),
    (1125, "div_MV"),
    (1126, "bin_NG"),
    (1127, "fuv_NG"),
    (1128, "ha_NG"),
    (1129, "ibb_NG"),
    (1130, "yo_NG"),
    (1131, "quz_BO"),
    (1132, "ns_ZA"),
    (1133, "ba_RU"),
    (1134, "lb_LU"),
    (1135, "kl_GL"),
    (1144, "ii_CN"),
    (1146, "arn_CL"),
    (1148, "moh_CA"),
    (1150, "br_FR"),
    (1152, "ug_CN"),
    (1153, "mi_NZ"),
    (1154, "oc_FR"),
    (1155, "co_FR"),
    (1156, "gsw_FR"),
    (1157, "sah_RU"),
    (1158, "qut_GT"),
    (1159, "rw_RW"),
    (1160, "wo_SN"),
    (1164, "gbz_AF"),
    (2049, "ar_IQ"),
    (2052, "zh_CN"),
    (2055, "de_CH"),
    (2057, "en_GB"),
    (2058, "es_MX"),
    (2060, "fr_BE"),
    (2064, "it_CH"),
    (2067, "nl_BE"),
    (2068, "nn_NO"),
    (2070, "pt_PT"),
    (2072, "ro_MD"),
    (2073, "ru_MD"),
    (2077, "sv_FI"),
    (2080, "ur_IN"),
    (2092, "az_AZ"),
    (2094, "dsb_DE"),
    (2107, "se_SE"),
    (2108, "ga_IE"),
    (2110, "ms_BN"),
    (2115, "uz_UZ"),
    (2128, "mn_CN"),
    (2129, "bo_BT"),
    (2141, "iu_CA"),
    (2143, "tmz_DZ"),
    (2145, "ne_IN"),
    (2155, "quz_EC"),
    (2163, "ti_ET"),
    (3073, "ar_EG"),
    (3076, "zh_HK"),
    (3079, "de_AT"),
    (3081, "en_AU"),
    (3082, "es_ES"),
    (3084, "fr_CA"),
    (3098, "sr_SP"),
    (3131, "se_FI"),
    (3179, "quz_PE"),
    (4097, "ar_LY"),
    (4100, "zh_SG"),
    (4103, "de_LU"),
    (4105, "en_CA"),
    (4106, "es_GT"),
    (4108, "fr_CH"),
    (4122, "hr_BA"),
    (4155, "smj_NO"),
    (5121, "ar_DZ"),
    (5124, "zh_MO"),
    (5127, "de_LI"),
    (5129, "en_NZ"),
    (5130, "es_CR"),
    (5132, "fr_LU"),
    (5179, "smj_SE"),
    (6145, "ar_MA"),
    (6153, "en_IE"),
    (6154, "es_PA"),
    (6156, "fr_MC"),
    (6203, "sma_NO"),
    (7169, "ar_TN"),
    (7177, "en_ZA"),
    (7178, "es_DO"),
    (7180, "fr_029"),
    (7194, "sr_BA"),
    (7227, "sma_SE"),
    (8193, "ar_OM"),
    (8201, "en_JA"),
    (8202, "es_VE"),
    (8204, "fr_RE"),
    (8218, "bs_BA"),
    (8251, "sms_FI"),
    (9217, "ar_YE"),
    (9225, "en_CB"),
    (9226, "es_CO"),
    (9228, "fr_CG"),
    (9275, "smn_FI"),
    (10241, "ar_SY"),
    (10249, "en_BZ"),
    (10250, "es_PE"),
    (10252, "fr_SN"),
    (11265, "ar_JO"),
    (11273, "en_TT"),
    (11274, "es_AR"),
    (11276, "fr_CM"),
    (12289, "ar_LB"),
    (12297, "en_ZW"),
    (12298, "es_EC"),
    (12300, "fr_CI"),
    (13313, "ar_KW"),
    (13321, "en_PH"),
    (13322, "es_CL"),
    (13324, "fr_ML"),
    (14337, "ar_AE"),
    (14345, "en_ID"),
    (14346, "es_UR"),
    (14348, "fr_MA"),
    (15361, "ar_BH"),
    (15369, "en_HK"),
    (15370, "es_PY"),
    (15372, "fr_HT"),
    (16385, "ar_QA"),
    (16393, "en_IN"),
    (16394, "es_BO"),
    (17417, "en_MY"),
    (17418, "es_SV"),
    (18441, "en_SG"),
    (18442, "es_HN"),
    (19466, "es_NI"),
    (20490, "es_PR"),
    (21514, "es_US"),
    (31748, "zh_CHT"),
];

/// The installer language LCID for a UI language code, or `None` if no
/// candidate's parts are prefixed by the input's parts.
pub fn language_to_installer_lcid(language: &str) -> Option<u32> {
    let language_parts: Vec<&str> = language.split('-').collect();
    for (lcid, candidate) in LCID_LANGUAGES {
        let candidate_parts: Vec<&str> = candidate.split('_').collect();
        if language_parts.len() <= candidate_parts.len()
            && language_parts
                .iter()
                .zip(candidate_parts.iter())
                .all(|(a, b)| a == b)
        {
            // Special case: use Spanish International for generic Spanish.
            return Some(if *lcid == 1034 { 3082 } else { *lcid });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_prefix_matches() {
        assert_eq!(language_to_installer_lcid("de"), Some(1031));
        assert_eq!(language_to_installer_lcid("de-DE"), Some(1031));
        assert_eq!(language_to_installer_lcid("en"), Some(1033));
        assert_eq!(language_to_installer_lcid("pt-BR"), Some(1046));
        assert_eq!(language_to_installer_lcid("pt-PT"), Some(2070));
    }

    #[test]
    fn spanish_is_remapped_to_international() {
        // es and es-ES both resolve to 1034, remapped to 3082.
        assert_eq!(language_to_installer_lcid("es"), Some(3082));
        assert_eq!(language_to_installer_lcid("es-ES"), Some(3082));
        // Other Spanish variants keep their own LCID.
        assert_eq!(language_to_installer_lcid("es-MX"), Some(2058));
    }

    #[test]
    fn unmatched_language_is_none() {
        assert_eq!(language_to_installer_lcid("xx-YY"), None);
        // Too many parts to be a prefix of any single/double-part candidate.
        assert_eq!(language_to_installer_lcid("en-US-extra"), None);
    }
}
