use kuchikiki::NodeRef;

use crate::types::Options;

/// Elements rendered inline (everything else is treated as a block).
const INLINE: &[&str] = &[
    "a", "b", "strong", "i", "em", "u", "span", "code", "small", "sub", "sup",
    "mark", "abbr", "cite", "q", "time", "label", "s", "del", "ins", "kbd",
    "var", "samp", "wbr", "big", "tt", "font", "img", "br",
];

struct Ctx<'a> {
    opts: &'a Options,
}

/// Convert a DOM subtree into Markdown.
pub fn to_markdown(root: &NodeRef, opts: &Options) -> String {
    let ctx = Ctx { opts };
    let body = render_children_as_blocks(root, &ctx, 0);
    normalize(&body)
}

/// Group a node's children: consecutive inline children become paragraphs,
/// block children are rendered as their own blocks.
fn render_children_as_blocks(node: &NodeRef, ctx: &Ctx, depth: usize) -> String {
    let mut out = String::new();
    let mut inline_buf = String::new();

    for child in node.children() {
        if is_block(&child) {
            flush_paragraph(&mut inline_buf, &mut out);
            out.push_str(&block_element(&child, ctx, depth));
        } else {
            inline_buf.push_str(&inline(&child, ctx));
        }
    }
    flush_paragraph(&mut inline_buf, &mut out);
    out
}

fn flush_paragraph(buf: &mut String, out: &mut String) {
    let text = buf.trim();
    if !text.is_empty() {
        out.push_str(text);
        out.push_str("\n\n");
    }
    buf.clear();
}

/// Render a block-level element to Markdown (with trailing blank line).
fn block_element(node: &NodeRef, ctx: &Ctx, depth: usize) -> String {
    let name = match node.as_element() {
        Some(e) => e.name.local.to_string(),
        None => return String::new(),
    };

    match name.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = name[1..].parse::<usize>().unwrap_or(1);
            let text = inline_children(node, ctx);
            let text = text.trim();
            if text.is_empty() {
                String::new()
            } else {
                format!("{} {}\n\n", "#".repeat(level), text)
            }
        }
        "p" => {
            let text = inline_children(node, ctx);
            let text = text.trim();
            if text.is_empty() {
                String::new()
            } else {
                format!("{text}\n\n")
            }
        }
        "ul" => render_list(node, ctx, false, depth),
        "ol" => render_list(node, ctx, true, depth),
        "dl" => render_dl(node, ctx, depth),
        "pre" => render_pre(node),
        "blockquote" => render_blockquote(node, ctx, depth),
        "table" => render_table(node, ctx),
        "hr" => "---\n\n".to_string(),
        "br" => String::new(),
        // Containers: recurse.
        _ => render_children_as_blocks(node, ctx, depth),
    }
}

/// Inline rendering for a single node (text or inline element).
fn inline(node: &NodeRef, ctx: &Ctx) -> String {
    if let Some(text) = node.as_text() {
        return collapse_ws(&text.borrow());
    }
    let name = match node.as_element() {
        Some(e) => e.name.local.to_string(),
        None => return String::new(),
    };

    match name.as_str() {
        "a" => {
            let text = inline_children(node, ctx);
            let text = text.trim();
            if !ctx.opts.include_links {
                return text.to_string();
            }
            let href = attr(node, "href").map(|h| resolve(&h, ctx)).unwrap_or_default();
            if href.is_empty() || text.is_empty() {
                text.to_string()
            } else {
                format!("[{text}]({href})")
            }
        }
        "img" => {
            if !ctx.opts.include_images {
                return String::new();
            }
            let src = match resolve_img_src(node, ctx) {
                Some(s) => s,
                None => return String::new(),
            };
            let alt = attr(node, "alt").unwrap_or_default();
            format!("![{}]({})", collapse_ws(&alt).trim(), src)
        }
        "strong" | "b" => wrap(node, ctx, "**"),
        "em" | "i" => wrap(node, ctx, "*"),
        "code" => {
            let text = node.text_contents();
            if text.is_empty() {
                String::new()
            } else {
                fence_inline_code(&text)
            }
        }
        "s" | "del" => wrap(node, ctx, "~~"),
        "br" => "  \n".to_string(),
        // Other inline elements: pass through their content.
        _ => inline_children(node, ctx),
    }
}

fn wrap(node: &NodeRef, ctx: &Ctx, marker: &str) -> String {
    let inner = inline_children(node, ctx);
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{marker}{trimmed}{marker}")
    }
}

fn inline_children(node: &NodeRef, ctx: &Ctx) -> String {
    let mut s = String::new();
    for child in node.children() {
        s.push_str(&inline(&child, ctx));
    }
    s
}

fn render_list(node: &NodeRef, ctx: &Ctx, ordered: bool, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let mut out = String::new();
    let mut i = 1;
    for li in node.children() {
        if element_name(&li).as_deref() != Some("li") {
            continue;
        }
        let mut text = String::new();
        let mut sublists = String::new();
        for c in li.children() {
            match element_name(&c).as_deref() {
                Some("ul") => sublists.push_str(&render_list(&c, ctx, false, depth + 1)),
                Some("ol") => sublists.push_str(&render_list(&c, ctx, true, depth + 1)),
                _ => text.push_str(&inline(&c, ctx)),
            }
        }
        let marker = if ordered {
            format!("{i}. ")
        } else {
            "- ".to_string()
        };
        out.push_str(&format!("{indent}{marker}{}\n", text.trim()));
        out.push_str(&sublists);
        i += 1;
    }
    if depth == 0 {
        out.push('\n');
    }
    out
}

/// Definition list: term in bold, each definition on a `: ` line (pandoc style).
///
/// `<dd>` bodies are rendered as blocks, not forced inline: API-reference
/// docs (e.g. Sphinx) routinely nest paragraphs, code examples, and even
/// sub-lists inside a single `<dd>`, and flattening those to one line of text
/// destroys code fences and structure entirely.
fn render_dl(node: &NodeRef, ctx: &Ctx, depth: usize) -> String {
    let mut out = String::new();
    for child in node.children() {
        match element_name(&child).as_deref() {
            Some("dt") => {
                let term = inline_children(&child, ctx);
                let term = term.trim();
                if !term.is_empty() {
                    out.push_str(&format!("**{term}**\n"));
                }
            }
            Some("dd") => {
                let body = render_children_as_blocks(&child, ctx, depth);
                let body = body.trim();
                if !body.is_empty() {
                    let mut lines = body.lines();
                    if let Some(first) = lines.next() {
                        out.push_str(&format!(": {first}\n"));
                    }
                    for line in lines {
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn render_pre(node: &NodeRef) -> String {
    // Language hint from a child <code class="language-xxx">.
    let mut lang = String::new();
    if let Ok(code) = node.select_first("code") {
        let attrs = code.attributes.borrow();
        if let Some(class) = attrs.get("class") {
            for tok in class.split_whitespace() {
                if let Some(l) = tok.strip_prefix("language-").or_else(|| tok.strip_prefix("lang-")) {
                    lang = l.to_string();
                    break;
                }
            }
        }
    }
    let code = node.text_contents();
    let code = code.trim_end_matches('\n');
    format!("```{lang}\n{code}\n```\n\n")
}

fn render_blockquote(node: &NodeRef, ctx: &Ctx, depth: usize) -> String {
    let inner = render_children_as_blocks(node, ctx, depth);
    let mut out = String::new();
    for line in inner.trim_end().lines() {
        if line.is_empty() {
            out.push_str(">\n");
        } else {
            out.push_str(&format!("> {line}\n"));
        }
    }
    out.push('\n');
    out
}

/// A source table cell before grid placement.
struct SrcCell {
    text: String,
    colspan: usize,
    rowspan: usize,
    is_th: bool,
}

fn render_table(node: &NodeRef, ctx: &Ctx) -> String {
    // 1. Collect source rows (cells with their spans).
    let mut src_rows: Vec<Vec<SrcCell>> = Vec::new();
    for tr in node.select("tr").map(|s| s.collect::<Vec<_>>()).unwrap_or_default() {
        let mut cells = Vec::new();
        for cell in tr.as_node().children() {
            let name = element_name(&cell);
            let is_th = match name.as_deref() {
                Some("th") => true,
                Some("td") => false,
                _ => continue,
            };
            let text = inline_children(&cell, ctx)
                .trim()
                .replace('|', "\\|")
                .replace('\n', " ");
            cells.push(SrcCell {
                text,
                colspan: span_attr(&cell, "colspan"),
                rowspan: span_attr(&cell, "rowspan"),
                is_th,
            });
        }
        if !cells.is_empty() {
            src_rows.push(cells);
        }
    }
    if src_rows.is_empty() {
        return String::new();
    }

    // 2. Lay cells onto a grid, honouring colspan (fills right) and rowspan
    //    (fills down; continuation cells are empty). Prevents column drift.
    use std::collections::HashMap;
    let mut grid: HashMap<(usize, usize), String> = HashMap::new();
    let mut occupied: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut max_col = 0usize;
    let mut max_row = 0usize;
    let mut header_th_row0 = true; // is every cell placed on row 0 a <th>?

    for (r, cells) in src_rows.iter().enumerate() {
        let mut c = 0usize;
        for cell in cells {
            while occupied.contains(&(r, c)) {
                c += 1;
            }
            for dr in 0..cell.rowspan {
                for dc in 0..cell.colspan {
                    occupied.insert((r + dr, c + dc));
                    max_row = max_row.max(r + dr);
                }
            }
            grid.insert((r, c), cell.text.clone());
            if r == 0 && !cell.is_th {
                header_th_row0 = false;
            }
            max_col = max_col.max(c + cell.colspan);
            c += cell.colspan;
        }
    }
    let cols = max_col;
    let nrows = max_row + 1;
    let _ = header_th_row0; // header is always grid row 0 (promoted if no <th>)

    // 3. Emit GFM table: row 0 is the header, then the separator, then the rest.
    let mut out = String::new();
    out.push_str(&emit_row(&grid, 0, cols));
    out.push_str(&format!("|{}\n", " --- |".repeat(cols.max(1))));
    for r in 1..nrows {
        out.push_str(&emit_row(&grid, r, cols));
    }
    out.push('\n');
    out
}

fn emit_row(grid: &std::collections::HashMap<(usize, usize), String>, r: usize, cols: usize) -> String {
    let mut s = String::from("|");
    for c in 0..cols.max(1) {
        let cell = grid.get(&(r, c)).map(|s| s.as_str()).unwrap_or("");
        s.push_str(&format!(" {cell} |"));
    }
    s.push('\n');
    s
}

/// Parse a `colspan`/`rowspan` attribute, defaulting to 1 and clamping to a
/// sane max so a hostile `colspan="100000"` can't blow up memory.
fn span_attr(node: &NodeRef, name: &str) -> usize {
    attr(node, name)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 100)
}

// ---- small helpers ----

fn is_block(node: &NodeRef) -> bool {
    match node.as_element() {
        Some(e) => !INLINE.contains(&e.name.local.as_ref()),
        None => false, // text nodes are inline
    }
}

fn element_name(node: &NodeRef) -> Option<String> {
    node.as_element().map(|e| e.name.local.to_string())
}

fn attr(node: &NodeRef, name: &str) -> Option<String> {
    node.as_element()
        .and_then(|e| e.attributes.borrow().get(name).map(|s| s.to_string()))
}

fn resolve(href: &str, ctx: &Ctx) -> String {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') {
        return href.to_string();
    }
    if let Some(base) = &ctx.opts.base_url {
        if let Ok(abs) = base.join(href) {
            return abs.to_string();
        }
    }
    href.to_string()
}

/// Wrap inline code so embedded backticks don't break the fence (CommonMark:
/// use one more backtick than the longest internal run; pad if it touches one).
fn fence_inline_code(text: &str) -> String {
    let mut longest = 0usize;
    let mut cur = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            cur += 1;
            longest = longest.max(cur);
        } else {
            cur = 0;
        }
    }
    let ticks = "`".repeat(longest + 1);
    if text.starts_with('`') || text.ends_with('`') {
        format!("{ticks} {text} {ticks}")
    } else {
        format!("{ticks}{text}{ticks}")
    }
}

/// Resolve an image URL from the many places sites stash it: lazy-load
/// attributes, then `src`, then the first `srcset` candidate. Skips `data:`
/// placeholders (usually 1px spacers or inline blobs that bloat output).
fn resolve_img_src(node: &NodeRef, ctx: &Ctx) -> Option<String> {
    for a in ["data-src", "data-original", "data-lazy-src", "src"] {
        if let Some(v) = attr(node, a) {
            let v = v.trim();
            if !v.is_empty() && !v.starts_with("data:") {
                return Some(resolve(v, ctx));
            }
        }
    }
    for a in ["srcset", "data-srcset"] {
        if let Some(v) = attr(node, a) {
            if let Some(first) = v.split(',').next() {
                let url = first.trim().split_whitespace().next().unwrap_or("");
                if !url.is_empty() && !url.starts_with("data:") {
                    return Some(resolve(url, ctx));
                }
            }
        }
    }
    None
}

/// Collapse any run of whitespace (incl. newlines) to a single space.
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

/// Collapse 3+ consecutive newlines to exactly 2, and trim trailing space.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newlines = 0;
    for line in s.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            newlines += 1;
            if newlines <= 1 {
                out.push('\n');
            }
        } else {
            newlines = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out.trim_end().to_string() + "\n"
}
