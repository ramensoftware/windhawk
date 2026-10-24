//! The user stylesheet: a `windhawk.css` in the folder holding `windhawk-ui.exe`,
//! applied over the front-end's own styles.
//!
//! The file is read once, as the window is built, and handed to the page by an
//! initialization script that adopts it as a constructed stylesheet. Adopted sheets
//! cascade after every sheet in the document, including the `<style>` tags the
//! front-end's CSS-in-JS adds at runtime, so a user rule wins a tie on specificity;
//! and the script runs before the document is parsed, so the rules hold from the
//! first paint. A constructed sheet takes no `@import`, and whatever its rules fetch
//! is subject to the page's CSP like any other style.

use std::io::ErrorKind;
use std::path::Path;

const FILE_NAME: &str = "windhawk.css";

/// The user stylesheet beside the running executable, or `None` where there is none.
pub fn load() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    read(&exe.parent()?.join(FILE_NAME))
}

/// The stylesheet at `path`. A missing file is the ordinary case and passes
/// silently; one that is there but cannot be read is reported, since whoever put it
/// there expects it to apply.
fn read(path: &Path) -> Option<String> {
    match std::fs::read(path) {
        Ok(bytes) => Some(decode(&bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            eprintln!("windhawk-ui: could not read {}: {error}", path.display());
            None
        }
    }
}

/// The file's text, decoded as a browser decodes a stylesheet that declares no
/// encoding: UTF-8, with a byte order mark dropped and invalid sequences replaced. A
/// mark left in would become part of the first selector, and that rule would match
/// nothing.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// The initialization script that adopts `css` as a stylesheet of every document the
/// window loads.
pub fn init_script(css: &str) -> String {
    match serde_json::to_string(css) {
        Ok(literal) => format!(
            "(function(){{var s=new CSSStyleSheet();s.replaceSync({literal});\
             document.adoptedStyleSheets=[...document.adoptedStyleSheets,s];}})();"
        ),
        // Unreachable for a string; an empty script leaves the page on its own styles.
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_in_place_is_read_and_anything_else_is_no_stylesheet() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join(FILE_NAME);

        assert_eq!(read(&path), None);

        std::fs::write(&path, "body{color:red}").unwrap();
        assert_eq!(read(&path).as_deref(), Some("body{color:red}"));

        // Present but unreadable: reported, and the page keeps its own styles.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(read(&path), None);
    }

    #[test]
    fn a_leading_byte_order_mark_is_dropped_and_bad_bytes_are_replaced() {
        assert_eq!(decode(b"\xEF\xBB\xBFbody{}"), "body{}");
        assert_eq!(decode(b"a{}\xEF\xBB\xBF"), "a{}\u{feff}");
        assert_eq!(decode(b"/*\xFF*/a{}"), "/*\u{fffd}*/a{}");
    }

    const SCRIPT_HEAD: &str = "(function(){var s=new CSSStyleSheet();s.replaceSync(";
    const SCRIPT_TAIL: &str =
        ");document.adoptedStyleSheets=[...document.adoptedStyleSheets,s];})();";

    #[test]
    fn the_init_script_adopts_the_css() {
        assert_eq!(
            init_script("a{b:c}"),
            format!(r#"{SCRIPT_HEAD}"a{{b:c}}"{SCRIPT_TAIL}"#)
        );
    }

    // The CSS is the user's text, so nothing in it may end the string it is carried
    // in: whatever it holds reaches the page unchanged.
    #[test]
    fn the_init_script_carries_any_css_verbatim() {
        let css = "a::before{content:\"\\\"x'\"}\r\n/* `${y}` */</style>\u{2028}\0";
        let script = init_script(css);
        let literal = script
            .strip_prefix(SCRIPT_HEAD)
            .and_then(|rest| rest.strip_suffix(SCRIPT_TAIL))
            .unwrap();

        assert_eq!(serde_json::from_str::<String>(literal).unwrap(), css);
    }
}
