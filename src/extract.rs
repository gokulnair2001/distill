use std::collections::HashMap;
use std::rc::Rc;

use kuchikiki::traits::*;
use kuchikiki::{Node, NodeRef};

/// Minimum characters for extracted content to be trusted; below this we fall
/// back to a larger container if one exists.
const MIN_CONTENT_CHARS: usize = 200;

/// Find the node most likely to contain the page's main content.
///
/// Readability-style scoring: each substantial text block scores points based on
/// length and comma count, and passes those points to its parent (full) and
/// grandparent (half). The winner is the scored element with the highest
/// link-density-adjusted score. Then we (a) merge in contentful sibling blocks
/// the scorer under-credited, and (b) fall back to a larger container if the
/// winner is suspiciously small.
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

    // Pick the best candidate, adjusting for link density. Track the raw score
    // too (used as the sibling-merge threshold).
    let mut best: Option<(f64, f64, NodeRef)> = None; // (adjusted, raw, node)
    for (ptr, raw_score) in &scores {
        let node = &nodes[ptr];
        let ld = link_density(node);
        let adjusted = raw_score * (1.0 - ld);
        if best.as_ref().map(|(s, _, _)| adjusted > *s).unwrap_or(true) {
            best = Some((adjusted, *raw_score, node.clone()));
        }
    }

    let (best_node, best_raw) = match best {
        Some((_, raw, node)) => (node, raw),
        None => return fallback_container(root, 0),
    };

    // Under-extraction guard: if the winner is tiny, prefer a bigger container.
    let best_len = inner_text(&best_node).chars().count();
    if best_len < MIN_CONTENT_CHARS {
        return fallback_container(root, best_len);
    }

    // Sibling merge: pull in adjacent blocks the scorer under-credited.
    merge_siblings(&best_node, best_raw, &scores)
}

/// Return `<main>`/`<article>`/`<body>` if it holds more text than `min_len`,
/// else the root. Used as a fallback when scoring finds too little.
fn fallback_container(root: &NodeRef, min_len: usize) -> NodeRef {
    for sel in ["main", "article", "body"] {
        if let Ok(el) = root.select_first(sel) {
            let node = el.as_node();
            if inner_text(node).chars().count() > min_len {
                return node.clone();
            }
        }
    }
    root.clone()
}

/// Gather the winner plus qualifying sibling elements into a fresh container.
/// Standard Readability behaviour: adjacent high-scoring blocks and dense
/// paragraphs belong to the same article even if the scorer split them.
fn merge_siblings(
    best: &NodeRef,
    best_raw: f64,
    scores: &HashMap<*const Node, f64>,
) -> NodeRef {
    let parent = match element_parent(best) {
        Some(p) => p,
        None => return best.clone(),
    };
    let threshold = (best_raw * 0.2).max(10.0);

    // Collect qualifying siblings (as Rc clones) before mutating the tree.
    let mut chosen: Vec<NodeRef> = Vec::new();
    for sib in parent.children() {
        if sib.as_element().is_none() {
            continue;
        }
        let is_best = Rc::ptr_eq(&sib.0, &best.0);
        let sib_score = scores.get(&Rc::as_ptr(&sib.0)).copied().unwrap_or(0.0);
        if is_best || sib_score >= threshold || is_contentful_paragraph(&sib) {
            chosen.push(sib.clone());
        }
    }

    if chosen.len() <= 1 {
        return best.clone();
    }

    let wrapper = new_container();
    for c in chosen {
        wrapper.append(c); // detaches from `parent`, re-parents into wrapper
    }
    wrapper
}

/// A detached `<div>` we can re-parent chosen content into for conversion.
fn new_container() -> NodeRef {
    let doc = kuchikiki::parse_html().one("<div id=\"__distill_content__\"></div>");
    doc.select_first("#__distill_content__")
        .map(|e| e.as_node().clone())
        .unwrap_or(doc)
}

fn is_contentful_paragraph(node: &NodeRef) -> bool {
    element_name(node).as_deref() == Some("p")
        && inner_text(node).chars().count() > 80
        && link_density(node) < 0.25
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
