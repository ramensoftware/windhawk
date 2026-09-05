//! The bound on how deep the settings document may nest, applied before the
//! tree is built.
//!
//! Every walk of a loaded tree descends a stack frame per level: the loader's
//! own recursive descent, its clone of an anchored node, the tree's drop, and
//! the validate, transform and flatten passes. Depth is cheap to author -
//! `- a:` on lines of growing indentation costs O(depth^2) bytes, so a 426 KB
//! block nests 1300 levels - and the overflow that follows is a guard-page
//! abort no `catch_unwind` at an ffi entry can contain, where every other
//! malformed block is an error the caller is told about. The depth is read off
//! the parser's event stream, which materializes nothing, a token at a time so
//! the scan itself never recurses.
//!
//! An alias makes the tree deeper than the document reads: the loader COPIES
//! the anchored node into the alias site, which lands there at the anchor's own
//! depth. Each alias is charged the deepest node the document has closed so
//! far, which covers whatever it resolves to. That over-states an alias to a
//! shallow anchor in a document that nests deeply elsewhere; no published block
//! uses an alias at all, and none nests past 15 levels.

use yaml_rust2::Event;
use yaml_rust2::parser::Parser;

/// Levels the loaded tree may reach. Several times over the deepest settings
/// block in the published catalog, which nests 15, and far short of the depth
/// at which a recursive walk of the tree runs out of stack.
const MAX_DEPTH: usize = 64;

const TOO_DEEP: &str = "YAML nesting is too deep";

/// Refuse a settings block whose tree would nest past `MAX_DEPTH`. Anything a
/// real settings tree does passes through untouched.
pub(super) fn reject_deep_nesting(block: &str) -> Result<(), &'static str> {
    measure(block).map(|_| ())
}

/// How deep the tree the block would load as reaches, or `TOO_DEEP` as soon as
/// the walk crosses the bound.
fn measure(block: &str) -> Result<usize, &'static str> {
    let mut scan = Scan::default();
    let mut parser = Parser::new_from_str(block);
    loop {
        // A parse error ends the walk: the loader is the one place that words a
        // syntax error, and it stops at the same event, so nothing it would
        // build afterwards is left unmeasured.
        let Ok((event, _)) = parser.next_token() else {
            return Ok(scan.deepest);
        };
        match event {
            Event::StreamEnd => return Ok(scan.deepest),
            Event::Scalar(..) => scan.close(1)?,
            Event::Alias(_) => {
                let copied = scan.deepest_node.max(1);
                scan.close(copied)?;
            }
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                scan.open.push(0);
                // The subtree opening here is at least this deep, so a runaway
                // descent is refused at the level that starts it rather than at
                // the leaf that proves it - which is also what keeps `open`
                // from growing with the input.
                let level = scan.open.len();
                scan.reached(level)?;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let deepest_member = scan.open.pop().unwrap_or(0);
                scan.close(deepest_member + 1)?;
            }
            _ => {}
        }
    }
}

/// The state of the walk: the collections open at the current point, and the
/// two maxima the bound is read from.
#[derive(Default)]
struct Scan {
    /// One entry per open collection: the height of its deepest member so far.
    open: Vec<usize>,
    /// Height of the tallest node closed so far, which bounds the height of
    /// whatever an alias copies.
    deepest_node: usize,
    /// Deepest level the tree reaches.
    deepest: usize,
}

impl Scan {
    /// Close a node of the given height at the current level: it bounds any
    /// alias to it, it may be the deepest level the tree reaches, and it counts
    /// as a member of the collection enclosing it.
    fn close(&mut self, height: usize) -> Result<(), &'static str> {
        self.deepest_node = self.deepest_node.max(height);
        self.reached(self.open.len() + height)?;
        if let Some(deepest_member) = self.open.last_mut() {
            *deepest_member = (*deepest_member).max(height);
        }
        Ok(())
    }

    fn reached(&mut self, depth: usize) -> Result<(), &'static str> {
        self.deepest = self.deepest.max(depth);
        if self.deepest > MAX_DEPTH {
            return Err(TOO_DEEP);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::{Yaml, YamlLoader};

    /// `levels` nested flow sequences around a scalar, two bytes a level. The
    /// scanner refuses past 255 flow levels, so this shape reaches the bound
    /// but cannot reach the depth the loader dies at.
    fn nested(levels: usize) -> String {
        format!("{}1{}", "[".repeat(levels - 1), "]".repeat(levels - 1))
    }

    /// Block style, the shape depth is cheap to author in and the one nothing
    /// caps: a sequence and a mapping per line, on lines of growing
    /// indentation, plus the null value the last `a:` takes.
    fn block_nested(lines: usize) -> String {
        let mut yaml = String::new();
        for i in 0..lines {
            yaml.push_str(&" ".repeat(2 * i));
            yaml.push_str("- a:\n");
        }
        yaml
    }

    /// An alias chain that deepens: each line wraps an alias to the line above
    /// in `nest` more flow sequences, so the copy the loader makes adds `nest`
    /// levels per line while the document itself never reads deeper than
    /// `nest + 2`.
    fn alias_ladder(lines: usize, nest: usize) -> String {
        let mut yaml = String::from("- a0: &x0 lol");
        for i in 1..=lines {
            let (open, close) = ("[".repeat(nest), "]".repeat(nest));
            yaml.push_str(&format!("\n- a{i}: &x{i} {open}*x{}{close}", i - 1));
        }
        yaml
    }

    fn depth(yaml: &Yaml) -> usize {
        1 + match yaml {
            Yaml::Array(items) => items.iter().map(depth).max().unwrap_or(0),
            Yaml::Hash(map) => map
                .iter()
                .map(|(key, value)| depth(key).max(depth(value)))
                .max()
                .unwrap_or(0),
            _ => 0,
        }
    }

    #[test]
    fn ordinary_documents_pass() {
        for yaml in [
            "- a: 1\n- b: hello",
            "- a: &v [1, 2, 3]\n- b: *v\n- c: *v",
            // A block that does not parse is the loader's to answer for.
            "- 'unterminated",
            &nested(MAX_DEPTH),
            &alias_ladder(4, 10),
        ] {
            assert_eq!(reject_deep_nesting(yaml), Ok(()), "refused {yaml:?}");
        }
    }

    #[test]
    fn one_level_past_the_bound_is_refused() {
        assert_eq!(reject_deep_nesting(&nested(MAX_DEPTH + 1)), Err(TOO_DEEP));
    }

    /// 1400 levels, past the depth the loader's own descent survives on a 1 MiB
    /// stack, so that the scan reaches a verdict at all is the point of it.
    #[test]
    fn a_document_the_loader_could_not_load_is_refused() {
        assert_eq!(reject_deep_nesting(&block_nested(700)), Err(TOO_DEEP));
    }

    /// The document reads twelve levels deep; the tree the loader would build
    /// from it is eighty-three, because every alias site is a copy of the line
    /// above.
    #[test]
    fn an_alias_chain_that_deepens_the_tree_is_refused() {
        assert_eq!(reject_deep_nesting(&alias_ladder(8, 10)), Err(TOO_DEEP));
    }

    /// Alias-free documents only: an alias site is charged an upper bound on
    /// what it copies, not the exact depth of it.
    #[test]
    fn the_measure_matches_the_tree_the_loader_builds() {
        for yaml in [
            "- a: 1\n- b: hello",
            "- a: [1, {b: {c: [d]}}]",
            &nested(20),
            &block_nested(20),
        ] {
            let loaded = YamlLoader::load_from_str(yaml).expect("loads");
            let built = loaded.iter().map(depth).max().unwrap_or(0);
            assert_eq!(measure(yaml), Ok(built), "misread {yaml:?}");
        }
    }
}
