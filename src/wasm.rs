//! WebAssembly bindings for the browser playground on the project site.
//!
//! Only the pure `distill_html` core is exposed — there is no network access in
//! the browser, so this converts HTML you already have (paste or `fetch`) into
//! Markdown. Built with `--features wasm` and `--no-default-features` so none of
//! the `net` pipeline (reqwest / headless browser) is pulled into the `.wasm`.

use wasm_bindgen::prelude::*;

use crate::types::Options;

/// Convert an HTML string to Markdown, including the YAML frontmatter block.
///
/// Mirrors the CLI's default options (links, images, and frontmatter on). This
/// is the exact same conversion the CLI and MCP server run — just compiled to
/// WebAssembly and executed client-side.
#[wasm_bindgen]
pub fn distill_html(html: &str) -> String {
    let opts = Options::default();
    let doc = crate::distill_html(html, &opts);
    doc.render(opts.frontmatter)
}
