//! Shared text renderers: the `mod show` / `repo show` metadata block and the
//! scalar value formatting their per-key formatters build on. Homing the
//! duplicated header plus Description/README tail here keeps the two `show`
//! outputs from drifting when either is tweaked.

use std::io::{self, Write};

use serde_json::Value;

/// Render a scalar JSON value the way JS `String(value)` does: strings
/// verbatim, booleans as `true`/`false`, numbers in canonical form. Shared by
/// the per-command value formatters.
pub(crate) fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => "null".to_owned(),
        other => other.to_string(),
    }
}

/// The five fixed metadata header lines shared by `mod show` and `repo show`. A
/// named-field borrow rather than four same-typed `&str` positionals so a caller
/// cannot silently transpose `name` / `version` / `author` past the type checker.
pub(crate) struct MetadataHeader<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    pub author: &'a str,
    pub architecture: Option<&'a [String]>,
}

/// Write the `ID:` / `Name:` / `Version:` / `Author:` lines, plus
/// `Architectures:` when the mod declares any. The caller fills the `version` it
/// wants (the resolved repo version for `repo show`, `metadata.version` for
/// `mod show`).
pub(crate) fn write_metadata_header(
    out: &mut dyn Write,
    header: &MetadataHeader,
) -> io::Result<()> {
    writeln!(out, "ID:            {}", header.id)?;
    writeln!(out, "Name:          {}", header.name)?;
    writeln!(out, "Version:       {}", header.version)?;
    writeln!(out, "Author:        {}", header.author)?;
    if let Some(arch) = header.architecture
        && !arch.is_empty()
    {
        writeln!(out, "Architectures: {}", arch.join(", "))?;
    }
    Ok(())
}

/// Write the blank-line-separated `Description:` and `README:` blocks shared by
/// both `show` outputs: the description is 2-space-indented per line, the readme
/// is emitted verbatim with a trailing newline added when it lacks one. Each
/// block is omitted when its text is absent or empty.
pub(crate) fn write_description_and_readme(
    out: &mut dyn Write,
    description: Option<&str>,
    readme: Option<&str>,
) -> io::Result<()> {
    if let Some(description) = description
        && !description.is_empty()
    {
        writeln!(out)?;
        writeln!(out, "Description:")?;
        let indented = description
            .split('\n')
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        writeln!(out, "{indented}")?;
    }
    if let Some(readme) = readme
        && !readme.is_empty()
    {
        writeln!(out)?;
        writeln!(out, "README:")?;
        write!(out, "{readme}")?;
        if !readme.ends_with('\n') {
            writeln!(out)?;
        }
    }
    Ok(())
}
