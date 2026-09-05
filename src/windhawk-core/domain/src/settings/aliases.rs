//! The bound on YAML alias expansion, applied before the tree is built.
//!
//! `YamlLoader` resolves an alias by COPYING the anchored node into the alias
//! site, so a document that anchors each level and references it nine times on
//! the next multiplies its node count per authored line: a few hundred bytes of
//! settings block reach billions of nodes and exhaust the host inside
//! `load_from_str` - before any validation rule gets to reject the document,
//! and as an allocation failure no `catch_unwind` can contain. Counting the
//! nodes over the parser's event stream sees the growth without materializing
//! any of it.

use std::collections::HashMap;

use yaml_rust2::Event;
use yaml_rust2::parser::{EventReceiver, Parser};

/// Headroom over the block's length. Declaring a node costs at least a byte of
/// source, so an alias-free document cannot reach its own length in nodes and
/// can never trip the bound; the headroom only keeps it clear of the shortest
/// documents, where a node and a byte are the same order.
const NODE_BUDGET_HEADROOM: usize = 1024;

/// Refuse a settings block whose aliases expand to a node count out of all
/// proportion to its size. Alias use that stays within the bound - and a block
/// with no alias at all - passes through untouched.
pub(super) fn reject_runaway_aliases(block: &str) -> Result<(), &'static str> {
    // An alias is spelled `*anchor`, so a block without that byte has none.
    if !block.contains('*') {
        return Ok(());
    }

    let mut counter = NodeCounter {
        budget: block.len() + NODE_BUDGET_HEADROOM,
        ..NodeCounter::default()
    };
    // Dropping the parse error is deliberate: the loader is the one place that
    // words a syntax error, and whatever the block expanded before reaching it
    // has been counted already.
    let _ = Parser::new_from_str(block).load(&mut counter, true);

    if counter.over_budget {
        return Err("YAML aliases expand to too many nodes");
    }
    Ok(())
}

/// Counts the nodes the loader would materialize, stopping at the budget. The
/// count of each anchored subtree is kept so an alias to it costs what its copy
/// would cost, which is what makes the multiplication visible a level before it
/// is paid for.
#[derive(Default)]
struct NodeCounter {
    budget: usize,
    total: usize,
    over_budget: bool,
    /// One entry per open collection: its anchor id, and `total` as it stood
    /// before the collection's own node, so the end event can size the subtree.
    open: Vec<(usize, usize)>,
    /// Node count of each completed anchored subtree, by anchor id.
    anchors: HashMap<usize, usize>,
}

impl NodeCounter {
    fn add(&mut self, nodes: usize, anchor_id: usize) {
        // A valid anchor id starts at 1; 0 means the node carries no anchor.
        if anchor_id > 0 {
            self.anchors.insert(anchor_id, nodes);
        }
        self.total = self.total.saturating_add(nodes);
        if self.total > self.budget {
            self.over_budget = true;
        }
    }
}

impl EventReceiver for NodeCounter {
    fn on_event(&mut self, ev: Event) {
        if self.over_budget {
            return;
        }
        match ev {
            Event::Scalar(_, _, anchor_id, _) => self.add(1, anchor_id),
            // An alias whose anchor is not complete yet (a self-reference)
            // resolves to a single `BadValue` in the loader, so it costs one.
            Event::Alias(anchor_id) => {
                let nodes = self.anchors.get(&anchor_id).copied().unwrap_or(1);
                self.add(nodes, 0);
            }
            Event::SequenceStart(anchor_id, _) | Event::MappingStart(anchor_id, _) => {
                self.open.push((anchor_id, self.total));
                self.add(1, 0);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((anchor_id, before)) = self.open.pop()
                    && anchor_id > 0
                {
                    self.anchors.insert(anchor_id, self.total - before);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::{Yaml, YamlLoader};

    /// The classic alias bomb: every level anchors a list of nine references to
    /// the level below it, multiplying the node count by nine per line.
    fn alias_bomb(levels: usize) -> String {
        let mut yaml = String::from("- a0: &a0 lol");
        for i in 1..=levels {
            let refs = vec![format!("*a{}", i - 1); 9].join(",");
            yaml.push_str(&format!("\n- a{i}: &a{i} [{refs}]"));
        }
        yaml
    }

    fn nodes(yaml: &Yaml) -> usize {
        match yaml {
            Yaml::Array(items) => 1 + items.iter().map(nodes).sum::<usize>(),
            Yaml::Hash(map) => 1 + map.iter().map(|(k, v)| nodes(k) + nodes(v)).sum::<usize>(),
            _ => 1,
        }
    }

    #[test]
    fn a_bomb_is_refused_at_a_size_the_loader_survives() {
        // Nine levels is 9^9 nodes; the bound has to trip long before that, so
        // this test must finish in milliseconds rather than minutes.
        assert!(alias_bomb(9).len() < 512);
        assert_eq!(
            reject_runaway_aliases(&alias_bomb(9)),
            Err("YAML aliases expand to too many nodes")
        );
    }

    #[test]
    fn ordinary_documents_pass() {
        for yaml in [
            "- a: 1\n- b: hello",
            "- a: &v [1, 2, 3]\n- b: *v\n- c: *v",
            // An alias to an incomplete anchor, and a block that does not
            // parse: both are the loader's to answer for.
            "- k: &a\n   - &b [*a]",
            "- 'unterminated *a",
        ] {
            assert_eq!(reject_runaway_aliases(yaml), Ok(()), "rejected {yaml:?}");
        }
    }

    #[test]
    fn the_count_matches_the_tree_the_loader_builds() {
        for yaml in [
            "- a: 1\n- b: hello",
            "- a: &v [1, 2, 3]\n- b: *v\n- c: *v",
            &alias_bomb(3),
        ] {
            let mut counter = NodeCounter {
                budget: usize::MAX,
                ..NodeCounter::default()
            };
            let mut parser = Parser::new_from_str(yaml);
            parser.load(&mut counter, true).expect("parses");
            let loaded: usize = YamlLoader::load_from_str(yaml)
                .expect("loads")
                .iter()
                .map(nodes)
                .sum();
            assert_eq!(counter.total, loaded, "miscounted {yaml:?}");
        }
    }
}
