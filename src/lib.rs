//! distill — convert any website into clean, agent-ready Markdown.
//!
//! Pipeline: fetch → metadata → clean → extract → convert.

pub mod clean;
pub mod convert;
pub mod extract;
pub mod fetch;
pub mod metadata;
pub mod types;

use anyhow::Result;
use kuchikiki::traits::*;
use url::Url;

pub use types::{Document, Options};

/// Convert raw HTML into a [`Document`]. Pure (no network) so it's easy to test.
pub fn distill_html(html: &str, opts: &Options) -> Document {
    // Parse once for metadata (needs the full <head> before cleanup).
    let dom = kuchikiki::parse_html().one(html);
    let mut doc = metadata::extract(&dom);

    let debug = std::env::var("DISTILL_DEBUG").is_ok();
    if debug {
        let body_len = dom
            .select_first("body")
            .map(|b| b.text_contents().chars().count())
            .unwrap_or(0);
        eprintln!("[debug] body text chars before clean: {body_len}");
    }

    // Clean the DOM, then pick the main content (or the whole thing in raw mode).
    clean::clean(&dom);
    if debug {
        let body_len = dom
            .select_first("body")
            .map(|b| b.text_contents().chars().count())
            .unwrap_or(0);
        eprintln!("[debug] body text chars after clean: {body_len}");
    }
    let content = if opts.raw {
        dom.select_first("body")
            .map(|b| b.as_node().clone())
            .unwrap_or(dom.clone())
    } else {
        extract::find_main_content(&dom)
    };

    doc.markdown = convert::to_markdown(&content, opts);
    doc
}

/// Fetch a URL and convert it. The final (post-redirect) URL becomes the base
/// for resolving relative links unless the caller already set one.
pub fn distill_url(url: &str, mut opts: Options) -> Result<Document> {
    let fetched = fetch::fetch(url)?;
    if opts.base_url.is_none() {
        opts.base_url = Some(fetched.final_url.clone());
    }
    Ok(distill_html(&fetched.html, &opts))
}

/// Parse a `--base` override into a URL.
pub fn parse_base(s: &str) -> Result<Url> {
    Ok(Url::parse(s)?)
}
