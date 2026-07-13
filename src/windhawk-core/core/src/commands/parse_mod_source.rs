//! `parseModSource`: parse metadata, readme, and initial settings out of mod
//! source code. Each section parses independently so one malformed block
//! doesn't hide the others; a failed section yields a per-section error string,
//! never a command failure.

use serde_json::Value;
use windhawk_core_domain as domain;
use windhawk_core_protocol::{
    AppendToModIdAndNameParams, ParseModSourceParams, ParsedModSource, ParsedModSourceErrors,
};

use crate::convert::{metadata_to_protocol, settings_to_protocol};
use crate::dispatch::decode_params;
use crate::error::CoreError;

// Pure, session-free handlers (`Handler::Stateless`): they read no session
// state, so they take only the request params.
pub fn run(params: Value) -> Result<Value, CoreError> {
    let params: ParseModSourceParams = decode_params("parseModSource", params)?;

    let mut result = ParsedModSource {
        metadata: None,
        readme: None,
        initial_settings: None,
        errors: ParsedModSourceErrors::default(),
    };

    match domain::extract_metadata(&params.source, &params.language) {
        Ok(metadata) => result.metadata = Some(metadata_to_protocol(metadata)),
        Err(e) => result.errors.metadata = Some(e.to_string()),
    }

    // Readme extraction has no failure mode: an absent or malformed block
    // is null, exactly like the TS implementation.
    result.readme = domain::extract_readme(&params.source);

    match domain::extract_initial_settings(&params.source, &params.language) {
        Ok(settings) => result.initial_settings = settings.map(settings_to_protocol),
        Err(e) => result.errors.initial_settings = Some(e.to_string()),
    }

    serde_json::to_value(&result)
        .map_err(|e| CoreError::internal(format!("parseModSource result serialization: {e}")))
}

/// `appendToModIdAndName`: a pure source transform (the new-mod / fork flows),
/// dispatch-direct into `domain` like `parseModSource`. Returns the transformed
/// source as a bare JSON string.
pub fn append_mod_id_and_name(params: Value) -> Result<Value, CoreError> {
    let params: AppendToModIdAndNameParams = decode_params("appendToModIdAndName", params)?;
    let transformed = domain::append_to_id_and_name(
        &params.source,
        params.append_to_id.as_deref(),
        params.append_to_name.as_deref(),
    );
    Ok(Value::String(transformed))
}
