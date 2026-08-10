use distill::distill_html;
use distill::types::Options;
use url::Url;

fn opts() -> Options {
    Options {
        base_url: Some(Url::parse("https://ex.com/docs/").unwrap()),
        ..Options::default()
    }
}

#[test]
fn agent_ready_is_opt_in_json_with_three_layers() {
    let html = r#"<html><head>
      <title>Widget Docs</title>
      <link rel="canonical" href="https://ex.com/docs/widget"/>
    </head><body><article>
      <h1>Widget Docs</h1>
      <p>See the <a href="guide">guide</a> before installing anything important.</p>
      <h2>Install</h2>
      <pre><code class="language-bash">npm i widget</code></pre>
      <h2>API</h2>
      <table><tr><th>Name</th><th>Type</th></tr>
      <tr><td>id</td><td>string</td></tr></table>
    </article></body></html>"#;

    let doc = distill_html(html, &opts());
    let plain = doc.render(true);
    assert!(plain.starts_with("---"), "default path stays Markdown: {plain}");
    assert!(!plain.contains("\"chunks\""), "default must not be JSON");

    let ars = doc.render_agent_ready();
    assert!(ars.contains("\"sectioned_markdown\""), "{ars}");
    assert!(ars.contains("\"chunks\""), "{ars}");
    assert!(ars.contains("\"schema\""), "{ars}");
    assert!(ars.contains("## Contents") || ars.contains("Contents"), "{ars}");
    assert!(ars.contains("https://ex.com/docs/guide"), "{ars}");
    assert!(ars.contains("npm i widget"), "{ars}");
    assert!(ars.contains("\"headers\""), "{ars}");
    assert!(ars.contains("chunk-"), "{ars}");
}
