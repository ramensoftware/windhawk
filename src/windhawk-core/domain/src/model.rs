//! Domain models for mod source parsing. Shapes deliberately mirror the
//! contract DTOs, but the types are distinct: the protocol crate is
//! self-contained and conversions live in the application crate.

/// A metadata-parse failure (the `extract_metadata` producer), surfaced in
/// `ParsedModSource.errors.metadata`. The message is PRIVATE - read it via
/// `Display`/`to_string()`, not a public field (the producer split that removes
/// the old `ModSourceError(pub String)` `.0` access). No consumer branches on
/// the failure class, so this is a thin newtype, not a taxonomy.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct MetadataError(String);

impl MetadataError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// An initial-settings parse failure (the `extract_initial_settings` /
/// `extract_initial_settings_for_engine` producers), surfaced in
/// `ParsedModSource.errors.initialSettings`. Like `MetadataError`, the message
/// is PRIVATE - read via `Display`/`to_string()`. Split from `MetadataError` by
/// producer so a metadata error cannot be misclassified as a settings error;
/// neither is a per-message taxonomy (nothing branches on the class).
///
/// `Display` owns the `Failed to parse settings: ` prefix and the stored message
/// is the bare cause, so the prefix appears exactly once however the error
/// reaches a caller: every producer is labeled without repeating itself, and a
/// consumer that adds the label too would double it.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("Failed to parse settings: {0}")]
pub struct SettingsParseError(String);

impl SettingsParseError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Parsed metadata block. Every field optional, matching `ModMetadata` of the
/// TypeScript implementation's `src/services/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModMetadata {
    pub id: Option<String>,
    pub version: Option<String>,
    pub github: Option<String>,
    pub twitter: Option<String>,
    pub homepage: Option<String>,
    pub compiler_options: Option<String>,
    pub license: Option<String>,
    pub donate_url: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub architecture: Option<Vec<String>>,
}

/// One item of the initial-settings tree, after language selection of the
/// `$name`/`$description`/`$options` annotations.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingItem {
    pub key: String,
    pub value: SettingValue,
    pub name: Option<String>,
    pub description: Option<String>,
    /// Display options in declaration order; each entry is the single
    /// `{value: label}` pair of the YAML option object.
    pub options: Option<Vec<(String, String)>>,
}

/// A setting value (dropped the unrepresentable `Null`: validation rejects the
/// float/out-of-range/null leaves it was meant for, so it was never
/// constructed).
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    NumberArray(Vec<serde_json::Number>),
    StringArray(Vec<String>),
    Settings(Vec<SettingItem>),
    SettingsArray(Vec<Vec<SettingItem>>),
}

/// A flattened engine setting value (the leaf type the engine settings store
/// holds): a 32-bit integer or a string. The `extractInitialSettingsForEngine`
/// flattening turns the structured settings tree into a flat name->value map of
/// these (booleans become 0/1, the same way the TS does); the install flow
/// migrates and writes them as the mod's initial `[Settings]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineSettingValue {
    Int(i32),
    Str(String),
}
