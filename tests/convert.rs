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
fn no_links_flag_keeps_text_only() {
    let mut o = opts();
    o.include_links = false;
    let html = r#"<body><article><p>Read the <a href="/x">documentation</a> carefully before you begin the setup.</p></article></body>"#;
    let out = distill_html(html, &o).markdown;
    assert!(out.contains("documentation"), "got: {out}");
    assert!(!out.contains("]("), "link markup leaked: {out}");
}
