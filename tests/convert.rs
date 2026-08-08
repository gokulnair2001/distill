use distill::types::Options;
use distill::distill_html;
use url::Url;

fn opts() -> Options {
    Options {
        base_url: Some(Url::parse("https://ex.com/docs/").unwrap()),
        ..Options::default()
    }
}

fn md(html: &str) -> String {
    distill_html(html, &opts()).markdown
}

#[test]
fn headings_and_paragraphs() {
    let out = md("<body><article><h1>Title</h1><p>Hello world this is a body paragraph with enough text to score.</p></article></body>");
    assert!(out.contains("# Title"));
    assert!(out.contains("Hello world"));
}

#[test]
fn resolves_relative_links_to_absolute() {
    let out = md(r#"<body><article><p>See the <a href="auth">auth guide</a> for more, it is quite important to read.</p></article></body>"#);
    assert!(
        out.contains("[auth guide](https://ex.com/docs/auth)"),
        "got: {out}"
    );
}

#[test]
fn tables_render_as_markdown() {
    let html = r#"<body><article>
      <p>Parameters are described in the following table which lists everything.</p>
      <table><tr><th>Name</th><th>Type</th></tr><tr><td>id</td><td>string</td></tr></table>
    </article></body>"#;
    let out = md(html);
    assert!(out.contains("| Name | Type |"), "got: {out}");
    assert!(out.contains("| --- | --- |"), "got: {out}");
    assert!(out.contains("| id | string |"), "got: {out}");
}

#[test]
fn code_block_keeps_language() {
    let html = r#"<body><article><p>Install it with npm using the command shown just below here.</p><pre><code class="language-bash">npm i x</code></pre></article></body>"#;
    let out = md(html);
    assert!(out.contains("```bash"), "got: {out}");
    assert!(out.contains("npm i x"), "got: {out}");
}

#[test]
fn strips_nav_footer_and_boilerplate() {
    let html = r#"<body>
      <nav><a href="/">Home</a></nav>
      <div class="cookie-banner">Accept cookies please and thank you very much indeed</div>
      <article><p>This is the real article content and it has enough words to win scoring.</p></article>
      <footer>Copyright notice goes here in the footer area of the page layout</footer>
    </body>"#;
    let out = md(html);
    assert!(out.contains("real article content"), "got: {out}");
    assert!(!out.contains("Home"), "nav leaked: {out}");
    assert!(!out.to_lowercase().contains("cookie"), "cookie leaked: {out}");
    assert!(!out.to_lowercase().contains("copyright"), "footer leaked: {out}");
}

#[test]
fn does_not_strip_html_root_by_class() {
    // Regression: Wikipedia puts `...-menu-...` classes on <html>; must not nuke the doc.
    let html = r#"<html class="vector-feature-main-menu-pinned-enabled"><body><article><p>Body content that must survive cleaning because html class matched menu.</p></article></body></html>"#;
    let out = md(html);
    assert!(out.contains("must survive"), "html root got stripped: {out}");
}

#[test]
fn output_is_deterministic() {
    let html = std::fs::read_to_string("tests/fixtures/sample.html").unwrap();
    let a = distill_html(&html, &opts()).render(true);
    let b = distill_html(&html, &opts()).render(true);
    assert_eq!(a, b);
}

#[test]
fn frontmatter_carries_metadata() {
    let html = r#"<html lang="en"><head><title>T</title><meta name="author" content="Jane"></head><body><article><p>Some sufficiently long body text to make the extractor happy here now.</p></article></body></html>"#;
    let doc = distill_html(html, &opts());
    let rendered = doc.render(true);
    assert!(rendered.starts_with("---\n"), "got: {rendered}");
    assert!(rendered.contains("author: Jane"), "got: {rendered}");
}

#[test]
fn captures_content_split_across_siblings() {
    // Sibling merge / container fallback must not drop adjacent content blocks.
    let html = r#"<body>
      <div><p>First block paragraph with sufficiently many words to score as real content number one here.</p></div>
      <div><p>Second block paragraph with sufficiently many words to score as real content number two here.</p></div>
    </body>"#;
    let out = md(html);
    assert!(out.contains("content number one"), "lost first block: {out}");
    assert!(out.contains("content number two"), "lost second block: {out}");
}

#[test]
fn definition_list_renders() {
    let html = r#"<body><article><p>Intro long enough to anchor the extractor for this small doc page here.</p>
      <dl><dt>GET</dt><dd>Requests a resource.</dd></dl></article></body>"#;
    let out = md(html);
    assert!(out.contains("**GET**"), "got: {out}");
    assert!(out.contains(": Requests a resource."), "got: {out}");
}

#[test]
fn table_colspan_rowspan_aligns() {
    let html = r#"<body><article>
      <p>Intro long enough to anchor the extractor so the table survives extraction here now.</p>
      <table>
        <tr><th>Env</th><th>Region</th><th>Value</th></tr>
        <tr><td rowspan="2">prod</td><td>us</td><td>10</td></tr>
        <tr><td>eu</td><td>20</td></tr>
        <tr><td colspan="2">all</td><td>30</td></tr>
      </table></article></body>"#;
    let out = md(html);
    // rowspan continuation keeps eu/20 in their columns.
    assert!(out.contains("|  | eu | 20 |"), "rowspan misaligned: {out}");
    // colspan leaves an empty second column, 30 stays in column 3.
    assert!(out.contains("| all |  | 30 |"), "colspan misaligned: {out}");
}

#[test]
fn inline_code_escapes_backticks() {
    let html = r#"<body><article><p>Use <code>a`b</code> here in this sufficiently long paragraph of text content.</p></article></body>"#;
    let out = md(html);
    assert!(out.contains("``a`b``"), "backtick not escaped: {out}");
}

#[test]
fn in_article_header_is_kept() {
    let html = r#"<body><article><header><h1>Kept Title</h1></header>
      <p>Body paragraph with enough words to anchor extraction for this article content here now.</p></article></body>"#;
    let out = md(html);
    assert!(out.contains("# Kept Title"), "in-article header dropped: {out}");
}

#[test]
fn image_prefers_srcset_over_data_placeholder() {
    let html = r#"<body><article><p>Paragraph long enough to anchor the extractor so the image survives here now.</p>
      <img srcset="/img/small.png 480w, /img/large.png 1024w" src="data:image/gif;base64,AAAA" alt="Diagram"></article></body>"#;
    let out = md(html);
    assert!(out.contains("![Diagram](https://ex.com/img/small.png)"), "got: {out}");
    assert!(!out.contains("data:image"), "data placeholder leaked: {out}");
}

#[test]
fn render_mode_parses() {
    use distill::RenderMode;
    assert_eq!(RenderMode::parse("never"), Some(RenderMode::Never));
    assert_eq!(RenderMode::parse("AUTO"), Some(RenderMode::Auto));
    assert_eq!(RenderMode::parse("always"), Some(RenderMode::Always));
    assert_eq!(RenderMode::parse("bogus"), None);
    assert_eq!(RenderMode::default(), RenderMode::Auto);
}

#[test]
fn no_links_flag_keeps_text_only() {
    let mut o = opts();
    o.include_links = false;
    let html = r#"<body><article><p>Read the <a href="/x">documentation</a> carefully before you begin the setup.</p></article></body>"#;
    let out = distill_html(html, &o).markdown;
    assert!(out.contains("documentation"), "got: {out}");
    assert!(!out.contains("]("), "link markup leaked: {out}");
}
