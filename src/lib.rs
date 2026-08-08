//! distill — convert any website into clean, agent-ready Markdown.
//!
//! Pipeline: fetch → (render?) → metadata → clean → extract → convert.

pub mod clean;
pub mod convert;
pub mod extract;
pub mod fetch;
pub mod metadata;
pub mod render;
pub mod ssrf;
pub mod types;

#[cfg(feature = "mcp")]
pub mod mcp;

use anyhow::Result;
use kuchikiki::traits::*;
use url::Url;

pub use types::{Document, Options, RenderMode};

/// Virtual-time budget (ms) handed to the headless browser when rendering.
const RENDER_BUDGET_MS: u32 = 8000;

/// Convert raw HTML into a [`Document`]. Pure (no network) so it's easy to test.
pub fn distill_html(html: &str, opts: &Options) -> Document {
    // Parse once for metadata (needs the full <head> before cleanup).
    let dom = kuchikiki::parse_html().one(html);
    let mut doc = metadata::extract(&dom);

    // Clean the DOM, then pick the main content (or the whole thing in raw mode).
    clean::clean(&dom);
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
/// for resolving relative links unless the caller already set one. Depending on
/// [`Options::render`], the page may be rendered with a headless browser first.
pub fn distill_url(url: &str, mut opts: Options) -> Result<Document> {
    let fetched = fetch::fetch(url)?;
    let final_url = fetched.final_url.clone();
    if opts.base_url.is_none() {
        opts.base_url = Some(final_url.clone());
    }
    let html = maybe_render(final_url.as_str(), fetched.html, &opts);
    Ok(distill_html(&html, &opts))
}

/// Decide whether to render with a headless browser, returning the HTML to use.
/// Falls back to the static HTML if rendering is disabled or unavailable.
fn maybe_render(url: &str, static_html: String, opts: &Options) -> String {
    let debug = std::env::var("DISTILL_DEBUG").is_ok();
    let should = match opts.render {
        RenderMode::Never => false,
        RenderMode::Always => true,
        RenderMode::Auto => needs_render(&static_html),
    };
    if !should {
        return static_html;
    }
    match render::render(url, RENDER_BUDGET_MS) {
        Some(rendered) => {
            if debug {
                eprintln!(
                    "[debug] rendered {} chars (static was {})",
                    rendered.len(),
                    static_html.len()
                );
            }
            rendered
        }
        None => {
            if debug {
                eprintln!("[debug] render requested but unavailable; using static HTML");
            }
            static_html
        }
    }
}

/// Heuristic: does this static HTML look like an under-rendered SPA shell?
fn needs_render(html: &str) -> bool {
    let dom = kuchikiki::parse_html().one(html);
    let body = match dom.select_first("body") {
        Ok(b) => b.as_node().clone(),
        Err(_) => return false,
    };
    let body_words = body.text_contents().split_whitespace().count();

    // A nearly-empty body almost certainly needs JS to populate.
    if body_words < 100 {
        return true;
    }

    // A known SPA mount point that is essentially empty while the body's text
    // comes from chrome/noscript => client-rendered content.
    for sel in ["#root", "#app", "#__next", "#__nuxt", "[data-reactroot]"] {
        if let Ok(el) = dom.select_first(sel) {
            let mount_words = el.as_node().text_contents().split_whitespace().count();
            if mount_words < 50 && mount_words * 3 < body_words {
                return true;
            }
        }
    }
    false
}

/// Parse a `--base` override into a URL.
pub fn parse_base(s: &str) -> Result<Url> {
    Ok(Url::parse(s)?)
}
