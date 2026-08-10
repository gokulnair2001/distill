//! Opt-in agent-ready structure: reshape clean Markdown into three layers
//! agents can consume without re-parsing a flat blob.
//!
//! 1. **sectioned_markdown** — normalized heading hierarchy + TOC
//! 2. **chunks** — RAG-oriented units (`heading` + `text` + `source`)
//! 3. **schema** — deterministic structured extract (meta, outline, links,
//!    tables, code blocks)
//!
//! Pure and deterministic — no LLM. Enabled only when the caller asks
//! (`--agent-ready` / `--ars`, or MCP `agent_ready: true`).

use crate::types::Document;

/// One RAG-oriented chunk of page content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub id: String,
    pub heading: Option<String>,
    pub level: Option<u8>,
    pub text: String,
    pub source: Option<String>,
}

/// Heading entry for the page outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub level: u8,
    pub text: String,
}

/// A hyperlink extracted from the Markdown body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkItem {
    pub text: String,
    pub url: String,
}

/// A GFM table parsed into headers + rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableItem {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlockItem {
    pub language: Option<String>,
    pub code: String,
}

/// Deterministic structured extract of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSchema {
    pub title: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub lang: Option<String>,
    pub canonical: Option<String>,
    pub site: Option<String>,
    pub outline: Vec<OutlineItem>,
    pub links: Vec<LinkItem>,
    pub tables: Vec<TableItem>,
    pub code_blocks: Vec<CodeBlockItem>,
}

/// All three agent-ready layers for a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReady {
    pub sectioned_markdown: String,
    pub chunks: Vec<Chunk>,
    pub schema: PageSchema,
}

/// Build the three-layer agent-ready structure from a distilled [`Document`].
pub fn build(doc: &Document) -> AgentReady {
    let sections = split_sections(&doc.markdown);
    let sectioned_markdown = render_sectioned(doc.title.as_deref(), &sections);
    let source = doc.canonical.clone();
    let chunks = sections_to_chunks(&sections, source.as_deref());
    let schema = PageSchema {
        title: doc.title.clone(),
        author: doc.byline.clone(),
        date: doc.date.clone(),
        lang: doc.lang.clone(),
        canonical: doc.canonical.clone(),
        site: doc.site.clone(),
        outline: sections
            .iter()
            .filter_map(|s| {
                s.heading.as_ref().map(|h| OutlineItem {
                    level: s.level.unwrap_or(1),
                    text: h.clone(),
                })
            })
            .collect(),
        links: extract_links(&doc.markdown),
        tables: extract_tables(&doc.markdown),
        code_blocks: extract_code_blocks(&doc.markdown),
    };
    AgentReady {
        sectioned_markdown,
        chunks,
        schema,
    }
}

/// Serialize [`AgentReady`] as pretty-printed JSON.
pub fn to_json(ready: &AgentReady) -> String {
    let mut out = String::from("{\n");
    out.push_str("  \"sectioned_markdown\": ");
    push_json_string(&mut out, &ready.sectioned_markdown);
    out.push_str(",\n  \"chunks\": [\n");
    for (i, c) in ready.chunks.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str("      \"id\": ");
        push_json_string(&mut out, &c.id);
        out.push_str(",\n      \"heading\": ");
        push_json_opt_string(&mut out, c.heading.as_deref());
        out.push_str(",\n      \"level\": ");
        match c.level {
            Some(n) => out.push_str(&n.to_string()),
            None => out.push_str("null"),
        }
        out.push_str(",\n      \"text\": ");
        push_json_string(&mut out, &c.text);
        out.push_str(",\n      \"source\": ");
        push_json_opt_string(&mut out, c.source.as_deref());
        out.push_str("\n    }");
        if i + 1 < ready.chunks.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ],\n  \"schema\": {\n");
    push_schema_json(&mut out, &ready.schema);
    out.push_str("  }\n}\n");
    out
}

/// Convenience: build + serialize in one step.
pub fn render(doc: &Document) -> String {
    to_json(&build(doc))
}

#[derive(Debug, Clone)]
struct Section {
    heading: Option<String>,
    level: Option<u8>,
    body: String,
}

/// Split Markdown into preamble + heading-bounded sections.
/// Respects fenced code blocks so `#` inside fences is not a heading.
fn split_sections(markdown: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut cur_heading: Option<String> = None;
    let mut cur_level: Option<u8> = None;
    let mut cur_body = String::new();
    let mut in_fence = false;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            push_body_line(&mut cur_body, line);
            continue;
        }
        if !in_fence {
            if let Some((level, text)) = parse_heading(line) {
                flush_section(&mut sections, cur_heading.take(), cur_level.take(), &mut cur_body);
                cur_heading = Some(text);
                cur_level = Some(level);
                continue;
            }
        }
        push_body_line(&mut cur_body, line);
    }
    flush_section(&mut sections, cur_heading, cur_level, &mut cur_body);

    if sections.is_empty() {
        sections.push(Section {
            heading: None,
            level: None,
            body: markdown.trim().to_string(),
        });
    }
    sections
}

fn parse_heading(line: &str) -> Option<(u8, String)> {
    let bytes = line.as_bytes();
    if bytes.is_empty() || bytes[0] != b'#' {
        return None;
    }
    let mut level = 0u8;
    for &b in bytes {
        if b == b'#' {
            level += 1;
            if level > 6 {
                return None;
            }
        } else {
            break;
        }
    }
    if level == 0 {
        return None;
    }
    let rest = line.get(level as usize..)?;
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        // ATX headings require space after hashes (CommonMark).
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some((level, text))
}

fn push_body_line(body: &mut String, line: &str) {
    if !body.is_empty() {
        body.push('\n');
    }
    body.push_str(line);
}

fn flush_section(
    sections: &mut Vec<Section>,
    heading: Option<String>,
    level: Option<u8>,
    body: &mut String,
) {
    let text = body.trim().to_string();
    body.clear();
    if heading.is_none() && text.is_empty() {
        return;
    }
    sections.push(Section {
        heading,
        level,
        body: text,
    });
}

fn render_sectioned(title: Option<&str>, sections: &[Section]) -> String {
    let mut out = String::new();
    // Only synthesize an H1 when the body has none — avoid duplicating the
    // page title when convert already emitted `# Title` from the DOM.
    let has_h1 = sections.iter().any(|s| s.level == Some(1));
    if let Some(t) = title {
        if !has_h1 {
            out.push_str("# ");
            out.push_str(t);
            out.push_str("\n\n");
        }
    }

    let outline: Vec<&Section> = sections.iter().filter(|s| s.heading.is_some()).collect();
    if !outline.is_empty() {
        out.push_str("## Contents\n\n");
        for s in &outline {
            let heading = s.heading.as_deref().unwrap_or("");
            let level = s.level.unwrap_or(1).max(1);
            // Indent relative to the shallowest heading in the outline.
            let min_level = outline
                .iter()
                .filter_map(|x| x.level)
                .min()
                .unwrap_or(1)
                .max(1);
            let depth = level.saturating_sub(min_level) as usize;
            for _ in 0..depth {
                out.push_str("  ");
            }
            out.push_str("- ");
            out.push_str(heading);
            out.push('\n');
        }
        out.push('\n');
    }

    for s in sections {
        if let (Some(h), Some(level)) = (&s.heading, s.level) {
            let hashes = "#".repeat(level.max(1).min(6) as usize);
            out.push_str(&hashes);
            out.push(' ');
            out.push_str(h);
            out.push_str("\n\n");
        }
        if !s.body.is_empty() {
            out.push_str(&s.body);
            out.push_str("\n\n");
        }
    }
    out.trim_end().to_string() + "\n"
}

fn sections_to_chunks(sections: &[Section], source: Option<&str>) -> Vec<Chunk> {
    sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.heading.is_some() || !s.body.trim().is_empty())
        .map(|(i, s)| {
            let mut text = String::new();
            if let Some(h) = &s.heading {
                let level = s.level.unwrap_or(1).max(1).min(6);
                text.push_str(&"#".repeat(level as usize));
                text.push(' ');
                text.push_str(h);
                if !s.body.is_empty() {
                    text.push_str("\n\n");
                    text.push_str(&s.body);
                }
            } else {
                text.push_str(&s.body);
            }
            Chunk {
                id: format!("chunk-{}", i + 1),
                heading: s.heading.clone(),
                level: s.level,
                text,
                source: source.map(|s| s.to_string()),
            }
        })
        .collect()
}

fn extract_links(markdown: &str) -> Vec<LinkItem> {
    let mut links = Vec::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Skip images: ![alt](url)
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
                // skip image
                if let Some(end) = find_md_link_end(&chars, i + 1) {
                    i = end;
                    continue;
                }
            }
            if chars[i] == '[' {
                if let Some((text, url, end)) = parse_md_link(&chars, i) {
                    links.push(LinkItem { text, url });
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
    }
    links
}

fn find_md_link_end(chars: &[char], start_bracket: usize) -> Option<usize> {
    // start_bracket points at '['
    let mut i = start_bracket + 1;
    while i < chars.len() && chars[i] != ']' {
        i += 1;
    }
    if i >= chars.len() || i + 1 >= chars.len() || chars[i + 1] != '(' {
        return None;
    }
    i += 2;
    while i < chars.len() && chars[i] != ')' {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    Some(i + 1)
}

fn parse_md_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    // start points at '['
    let mut i = start + 1;
    let text_start = i;
    while i < chars.len() && chars[i] != ']' {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let text: String = chars[text_start..i].iter().collect();
    i += 1;
    if i >= chars.len() || chars[i] != '(' {
        return None;
    }
    i += 1;
    let url_start = i;
    while i < chars.len() && chars[i] != ')' {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let url: String = chars[url_start..i].iter().collect();
    let url = url.split_whitespace().next().unwrap_or("").to_string();
    if url.is_empty() {
        return None;
    }
    Some((text, url, i + 1))
}

fn extract_code_blocks(markdown: &str) -> Vec<CodeBlockItem> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("```") {
            continue;
        }
        let lang = trimmed.trim_start_matches('`').trim();
        let language = if lang.is_empty() {
            None
        } else {
            Some(lang.to_string())
        };
        let mut code = String::new();
        for body in lines.by_ref() {
            if body.trim_start().starts_with("```") {
                break;
            }
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(body);
        }
        blocks.push(CodeBlockItem { language, code });
    }
    blocks
}

fn extract_tables(markdown: &str) -> Vec<TableItem> {
    let mut tables = Vec::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;
    let mut in_fence = false;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            i += 1;
            continue;
        }
        if in_fence {
            i += 1;
            continue;
        }
        if looks_like_table_row(line) && i + 1 < lines.len() && looks_like_separator(lines[i + 1]) {
            let headers = split_table_row(line);
            i += 2; // skip header + separator
            let mut rows = Vec::new();
            while i < lines.len() && looks_like_table_row(lines[i]) {
                rows.push(split_table_row(lines[i]));
                i += 1;
            }
            tables.push(TableItem { headers, rows });
            continue;
        }
        i += 1;
    }
    tables
}

fn looks_like_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('|')
}

fn looks_like_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with('|') {
        return false;
    }
    t.chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
        && t.contains('-')
}

fn split_table_row(line: &str) -> Vec<String> {
    let t = line.trim().trim_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_json_opt_string(out: &mut String, s: Option<&str>) {
    match s {
        Some(v) => push_json_string(out, v),
        None => out.push_str("null"),
    }
}

fn push_schema_json(out: &mut String, schema: &PageSchema) {
    out.push_str("    \"title\": ");
    push_json_opt_string(out, schema.title.as_deref());
    out.push_str(",\n    \"author\": ");
    push_json_opt_string(out, schema.author.as_deref());
    out.push_str(",\n    \"date\": ");
    push_json_opt_string(out, schema.date.as_deref());
    out.push_str(",\n    \"lang\": ");
    push_json_opt_string(out, schema.lang.as_deref());
    out.push_str(",\n    \"canonical\": ");
    push_json_opt_string(out, schema.canonical.as_deref());
    out.push_str(",\n    \"site\": ");
    push_json_opt_string(out, schema.site.as_deref());

    out.push_str(",\n    \"outline\": [\n");
    for (i, item) in schema.outline.iter().enumerate() {
        out.push_str("      {\"level\": ");
        out.push_str(&item.level.to_string());
        out.push_str(", \"text\": ");
        push_json_string(out, &item.text);
        out.push('}');
        if i + 1 < schema.outline.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("    ],\n    \"links\": [\n");
    for (i, link) in schema.links.iter().enumerate() {
        out.push_str("      {\"text\": ");
        push_json_string(out, &link.text);
        out.push_str(", \"url\": ");
        push_json_string(out, &link.url);
        out.push('}');
        if i + 1 < schema.links.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("    ],\n    \"tables\": [\n");
    for (i, table) in schema.tables.iter().enumerate() {
        out.push_str("      {\n        \"headers\": [");
        for (j, h) in table.headers.iter().enumerate() {
            push_json_string(out, h);
            if j + 1 < table.headers.len() {
                out.push_str(", ");
            }
        }
        out.push_str("],\n        \"rows\": [\n");
        for (r, row) in table.rows.iter().enumerate() {
            out.push_str("          [");
            for (c, cell) in row.iter().enumerate() {
                push_json_string(out, cell);
                if c + 1 < row.len() {
                    out.push_str(", ");
                }
            }
            out.push(']');
            if r + 1 < table.rows.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("        ]\n      }");
        if i + 1 < schema.tables.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("    ],\n    \"code_blocks\": [\n");
    for (i, block) in schema.code_blocks.iter().enumerate() {
        out.push_str("      {\"language\": ");
        push_json_opt_string(out, block.language.as_deref());
        out.push_str(", \"code\": ");
        push_json_string(out, &block.code);
        out.push('}');
        if i + 1 < schema.code_blocks.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("    ]\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(md: &str) -> Document {
        Document {
            title: Some("Demo Page".into()),
            byline: Some("Ada".into()),
            date: Some("2026-08-10".into()),
            lang: Some("en".into()),
            canonical: Some("https://ex.com/demo".into()),
            site: Some("ex.com".into()),
            markdown: md.into(),
        }
    }

    #[test]
    fn splits_on_headings_not_inside_fences() {
        let md = "# Intro\n\nHello.\n\n## Install\n\n```bash\n# not a heading\nnpm i x\n```\n\n## Usage\n\nRun it.\n";
        let sections = split_sections(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].heading.as_deref(), Some("Intro"));
        assert_eq!(sections[1].heading.as_deref(), Some("Install"));
        assert!(sections[1].body.contains("# not a heading"));
        assert_eq!(sections[2].heading.as_deref(), Some("Usage"));
    }

    #[test]
    fn builds_all_three_layers() {
        let md = r#"# Intro

See the [guide](https://ex.com/guide) for details.

## API

| Name | Type |
| --- | --- |
| id | string |

```rust
fn main() {}
```
"#;
        let ready = build(&doc_with(md));
        assert!(ready.sectioned_markdown.contains("## Contents"));
        assert!(ready.sectioned_markdown.contains("- Intro"));
        assert!(ready.chunks.len() >= 2);
        assert_eq!(ready.chunks[0].source.as_deref(), Some("https://ex.com/demo"));
        assert_eq!(ready.schema.title.as_deref(), Some("Demo Page"));
        assert_eq!(ready.schema.links.len(), 1);
        assert_eq!(ready.schema.links[0].url, "https://ex.com/guide");
        assert_eq!(ready.schema.tables.len(), 1);
        assert_eq!(ready.schema.tables[0].headers, vec!["Name", "Type"]);
        assert_eq!(ready.schema.code_blocks.len(), 1);
        assert_eq!(ready.schema.code_blocks[0].language.as_deref(), Some("rust"));
    }

    #[test]
    fn json_round_trips_key_fields() {
        let ready = build(&doc_with(
            "## Only\n\nBody with [a](https://ex.com/a).\n",
        ));
        let json = to_json(&ready);
        assert!(json.contains("\"sectioned_markdown\""));
        assert!(json.contains("\"chunks\""));
        assert!(json.contains("\"schema\""));
        assert!(json.contains("chunk-1"));
        assert!(json.contains("https://ex.com/a"));
        // Default Markdown path must remain unchanged elsewhere; this is opt-in JSON.
        assert!(json.starts_with('{'));
    }

    #[test]
    fn no_heading_page_still_yields_one_chunk() {
        let ready = build(&doc_with("Just a paragraph with enough text.\n"));
        assert_eq!(ready.chunks.len(), 1);
        assert!(ready.chunks[0].heading.is_none());
        assert!(ready.schema.outline.is_empty());
    }
}
