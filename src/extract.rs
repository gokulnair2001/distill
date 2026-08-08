use std::collections::HashMap;
use std::rc::Rc;

use kuchikiki::{Node, NodeRef};

/// Find the node most likely to contain the page's main content.
///
/// Readability-style scoring: each substantial text block scores points based on
/// length and comma count, and passes those points to its parent (full) and
/// grandparent (half). The winner is the scored element with the highest
/// link-density-adjusted score. This deliberately avoids `<body>` winning by
/// default, since points only propagate two levels up.
pub fn find_main_content(root: &NodeRef) -> NodeRef {
    let mut scores: HashMap<*const Node, f64> = HashMap::new();
    let mut nodes: HashMap<*const Node, NodeRef> = HashMap::new();

    for el in root.select("p, pre, blockquote, td, article, section").unwrap() {
        let node = el.as_node();
        let text = inner_text(node);
        let len = text.chars().count();
        if len < 25 {
            continue;
        }

        let mut base = 1.0;
        base += text.matches(',').count() as f64;
        base += (len as f64 / 100.0).min(3.0);

        // Semantic containers get a small intrinsic boost.
        if let Some(name) = element_name(node) {
            if name == "article" {
                base += 5.0;
            } else if name == "section" {
                base += 1.0;
            }
        }

        add_score(&mut scores, &mut nodes, node, base);
        if let Some(parent) = element_parent(node) {
            add_score(&mut scores, &mut nodes, &parent, base);
            if let Some(gp) = element_parent(&parent) {
                add_score(&mut scores, &mut nodes, &gp, base / 2.0);
            }
        }
    }

    // Pick the best candidate, adjusting for link density.
    let mut best: Option<(f64, NodeRef)> = None;
    for (ptr, raw_score) in &scores {
        let node = &nodes[ptr];
        let ld = link_density(node);
        let adjusted = raw_score * (1.0 - ld);
        if best.as_ref().map(|(s, _)| adjusted > *s).unwrap_or(true) {
            best = Some((adjusted, node.clone()));
        }
    }

    if let Some((_, node)) = best {
        return node;
    }

    // Fallbacks: <main>, <article>, then <body>, then root.
    for sel in ["main", "article", "body"] {
        if let Ok(el) = root.select_first(sel) {
            return el.as_node().clone();
        }
    }
    root.clone()
}

fn add_score(
    scores: &mut HashMap<*const Node, f64>,
    nodes: &mut HashMap<*const Node, NodeRef>,
    node: &NodeRef,
    delta: f64,
) {
    let ptr = Rc::as_ptr(&node.0);
    *scores.entry(ptr).or_insert(0.0) += delta;
    nodes.entry(ptr).or_insert_with(|| node.clone());
}

/// Nearest ancestor that is an element node.
fn element_parent(node: &NodeRef) -> Option<NodeRef> {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if n.as_element().is_some() {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

fn element_name(node: &NodeRef) -> Option<String> {
    node.as_element().map(|e| e.name.local.to_string())
}

fn inner_text(node: &NodeRef) -> String {
    node.text_contents()
}

/// Fraction of a node's text that sits inside `<a>` elements (0.0–1.0).
fn link_density(node: &NodeRef) -> f64 {
    let total = inner_text(node).chars().count();
    if total == 0 {
        return 0.0;
    }
    let mut link_len = 0usize;
    if let Ok(links) = node.select("a") {
        for a in links {
            link_len += a.as_node().text_contents().chars().count();
        }
    }
    (link_len as f64 / total as f64).min(1.0)
}
