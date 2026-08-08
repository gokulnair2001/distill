use url::Url;

/// When to use a headless browser to render JavaScript before extracting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// Never render; static fetch only (fastest).
    Never,
    /// Render only when the static HTML looks under-rendered (SPA shell).
    #[default]
    Auto,
    /// Always render with a headless browser.
    Always,
}

impl RenderMode {
    pub fn parse(s: &str) -> Option<RenderMode> {
        match s.to_ascii_lowercase().as_str() {
            "never" | "off" | "none" => Some(RenderMode::Never),
            "auto" => Some(RenderMode::Auto),
            "always" | "on" | "force" => Some(RenderMode::Always),
            _ => None,
        }
    }
}

/// Conversion options controlling what ends up in the Markdown.
#[derive(Debug, Clone)]
pub struct Options {
    /// Keep hyperlinks as `[text](url)`. Agents usually want these for navigation.
    pub include_links: bool,
    /// Keep images as `![alt](src)`.
    pub include_images: bool,
    /// Emit a YAML frontmatter block with metadata.
    pub frontmatter: bool,
    /// Skip main-content extraction and convert the whole cleaned body (debug/raw mode).
    pub raw: bool,
    /// Whether/when to render JavaScript with a headless browser.
    pub render: RenderMode,
    /// Base URL used to resolve relative links/images to absolute URLs.
    pub base_url: Option<Url>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            include_links: true,
            include_images: true,
            frontmatter: true,
            raw: false,
            render: RenderMode::default(),
            base_url: None,
        }
    }
}

/// The parsed result of a page.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub date: Option<String>,
    pub lang: Option<String>,
    pub canonical: Option<String>,
    pub site: Option<String>,
    pub markdown: String,
}

impl Document {
    /// Render the document as a single Markdown string, optionally with frontmatter.
    pub fn render(&self, frontmatter: bool) -> String {
        if !frontmatter {
            return self.markdown.clone();
        }
        let mut out = String::new();
        let mut fields: Vec<(&str, &Option<String>)> = vec![
            ("title", &self.title),
            ("author", &self.byline),
            ("date", &self.date),
            ("lang", &self.lang),
            ("canonical", &self.canonical),
            ("site", &self.site),
        ];
        // Only emit frontmatter if we actually have something.
        if fields.iter().any(|(_, v)| v.is_some()) {
            out.push_str("---\n");
            for (k, v) in fields.drain(..) {
                if let Some(val) = v {
                    let escaped = yaml_escape(val);
                    out.push_str(&format!("{k}: {escaped}\n"));
                }
            }
            out.push_str("---\n\n");
        }
        out.push_str(&self.markdown);
        out
    }
}

/// Minimal YAML scalar escaping: quote when the value contains characters that
/// would break a plain scalar.
fn yaml_escape(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.contains(':')
        || s.contains('#')
        || s.contains('"')
        || s.contains('\'')
        || s.starts_with(|c: char| "-?[]{}&*!|>%@`".contains(c))
        || s.starts_with(char::is_whitespace)
        || s.ends_with(char::is_whitespace);
    if needs_quote {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
