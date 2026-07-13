//! `source meta <file>`: extract metadata from a `.wh.cpp` file. Operates on
//! the file directly with no Windhawk environment - it reads the file itself
//! and parses through the SESSION-FREE stateless transport, so no app root is
//! resolved and no session is created.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_host::GatedCore;
use windhawk_core_protocol::{ModMetadata, ParseModSourceParams, ParsedModSource};

use crate::commands::parse::require_metadata;
use crate::commands::render::scalar_to_string;
use crate::error::CliError;
use crate::output::{CommandResult, to_value};

pub fn meta(core: &GatedCore, file: &str) -> Result<Box<dyn CommandResult>, CliError> {
    let source = std::fs::read_to_string(file)
        .map_err(|e| CliError::usage(format!("Failed to read '{file}': {e}")))?;

    // parseModSource is a pure helper; the stateless variant keeps `source meta`
    // independent of environment discovery, which it explicitly does not need.
    let parsed: ParsedModSource = core.invoke_stateless_as(
        "parseModSource",
        &ParseModSourceParams {
            source,
            language: "en".to_owned(),
        },
    )?;

    // A file that does not parse is a usage error (exit 2), not a generic one:
    // unlike the stored-source commands, `source meta` validates an arbitrary
    // file the user pointed at, so the parse error is wrapped with the file name.
    let metadata = require_metadata(parsed.metadata, parsed.errors.metadata, |message| {
        CliError::usage(format!("Failed to parse metadata from '{file}': {message}"))
    })?;

    Ok(Box::new(SourceMetaResult { metadata }))
}

struct SourceMetaResult {
    metadata: ModMetadata,
}

impl CommandResult for SourceMetaResult {
    fn json_data(&self) -> Value {
        json!({ "metadata": self.metadata })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let value = to_value(&self.metadata);
        if let Some(obj) = value.as_object() {
            for (key, value) in obj {
                let rendered = match value {
                    Value::Array(items) => items
                        .iter()
                        .map(scalar_to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                    other => scalar_to_string(other),
                };
                writeln!(out, "{key}: {rendered}")?;
            }
        }
        Ok(())
    }
}

/// Golden (snapshot) tests of the compute-then-render seam for `source meta`:
/// the `key: value` listing and the `--json` `{metadata}` envelope, with no
/// file or session.
///
/// Field ORDER note (recorded deviation): the listing follows the typed
/// `ModMetadata` struct's serialization order, NOT the source-file declaration
/// order the TS `Object.entries` preserved. This is an accepted consequence of
/// the typed-DTO design (the CLI consumes typed wire DTOs, not raw `Value`, so
/// the source order is not recoverable); field order is not pinned for `source
/// meta`. Absent fields are skipped (serde `skip_serializing_if`), and arrays
/// comma-join.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    #[test]
    fn source_meta_lists_present_fields_in_struct_order() {
        let result = SourceMetaResult {
            metadata: ModMetadata {
                id: Some("smoke-mod".to_owned()),
                name: Some("Smoke Mod".to_owned()),
                version: Some("1.2.3".to_owned()),
                author: Some("Tester".to_owned()),
                architecture: Some(vec!["x86".to_owned(), "x86-64".to_owned()]),
                ..Default::default()
            },
        };
        // version/id precede name/author (struct order); architecture comma-joins.
        assert_eq!(
            render_text(&result),
            "version: 1.2.3\nid: smoke-mod\nname: Smoke Mod\nauthor: Tester\narchitecture: x86, x86-64\n"
        );
        assert_eq!(result.json_data()["metadata"]["id"], json!("smoke-mod"));
    }
}
