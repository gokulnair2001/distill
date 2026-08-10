//! MCP server: exposes `distill` as two Model Context Protocol tools over
//! stdio so any local agent can turn a URL or raw HTML into agent-ready
//! Markdown without shelling out.
//!
//! Tools:
//! - `distill_url`  — fetch a URL (SSRF-guarded) and convert it.
//! - `distill_html` — convert HTML the caller already has (no network).
//!
//! Conversion runs on a blocking thread (`spawn_blocking`): the fetch path is
//! synchronous and the HTML DOM is not `Send`, so it must not straddle an
//! await point or block the async reactor.

use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Semaphore;
use url::Url;

use crate::types::{Options, RenderMode};

/// Largest batch `distill_urls` will accept in a single call.
const MAX_BATCH: usize = 20;
/// How many URLs in a batch are fetched concurrently.
const BATCH_CONCURRENCY: usize = 4;

/// Conversion knobs shared by both tools. All optional; omitted fields take the
/// same defaults as the CLI (links/images/frontmatter on, raw off).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ConvOptions {
    /// Keep hyperlinks as `[text](url)`. Default: true.
    pub include_links: Option<bool>,
    /// Keep images as `![alt](src)`. Default: true.
    pub include_images: Option<bool>,
    /// Prepend a YAML frontmatter block with page metadata. Default: true.
    /// Ignored when `agent_ready` is true (metadata lives in the JSON schema).
    pub frontmatter: Option<bool>,
    /// Skip main-content extraction; convert the whole cleaned page. Default: false.
    pub raw: Option<bool>,
    /// Base URL for resolving relative links/images to absolute URLs.
    pub base: Option<String>,
    /// When true, return agent-ready JSON (sectioned Markdown + RAG chunks +
    /// schema) instead of plain Markdown. Default: false.
    pub agent_ready: Option<bool>,
}

/// Arguments for `distill_url`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UrlParams {
    /// Absolute `http(s)` URL to fetch and convert to Markdown.
    pub url: String,
    /// JavaScript rendering: `never` | `auto` | `always`. Default: `auto`
    /// (render with a headless browser only when the page looks under-rendered).
    pub render: Option<String>,
    #[serde(flatten)]
    pub options: ConvOptions,
}

/// Arguments for `distill_urls` (batch).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UrlsParams {
    /// Absolute `http(s)` URLs to fetch and convert. At most 20 per call.
    pub urls: Vec<String>,
    /// JavaScript rendering applied to every URL: `never` | `auto` | `always`.
    /// Default: `auto`.
    pub render: Option<String>,
    #[serde(flatten)]
    pub options: ConvOptions,
}

/// Arguments for `distill_html`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct HtmlParams {
    /// Raw HTML to convert to Markdown.
    pub html: String,
    #[serde(flatten)]
    pub options: ConvOptions,
}

/// The distill MCP server.
#[derive(Clone)]
pub struct DistillServer {
    tool_router: ToolRouter<Self>,
}

impl Default for DistillServer {
    fn default() -> Self {
        Self::new()
    }
}

impl DistillServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl DistillServer {
    #[tool(
        description = "Fetch a URL and convert the page into clean, agent-ready Markdown. \
        Boilerplate is stripped, structure (tables, code, nested lists) preserved, links \
        resolved to absolute URLs, and page metadata included as YAML frontmatter. Set \
        agent_ready=true for JSON with sectioned Markdown, RAG chunks, and a structured \
        schema. Requests to private/loopback/link-local addresses are refused (SSRF guard)."
    )]
    async fn distill_url(
        &self,
        Parameters(p): Parameters<UrlParams>,
    ) -> Result<CallToolResult, McpError> {
        let render = parse_render(p.render.as_deref())?;
        let opts = build_options(&p.options, render)?;
        let frontmatter = opts.frontmatter;
        let agent_ready = p.options.agent_ready.unwrap_or(false);
        let url = p.url;

        let markdown = tokio::task::spawn_blocking(move || {
            crate::distill_url(&url, opts).map(|doc| {
                if agent_ready {
                    doc.render_agent_ready()
                } else {
                    doc.render(frontmatter)
                }
            })
        })
        .await
        .map_err(|e| McpError::internal_error(format!("conversion task failed: {e}"), None))?
        .map_err(|e| McpError::internal_error(format!("distill failed: {e:#}"), None))?;

        Ok(CallToolResult::success(vec![ContentBlock::text(markdown)]))
    }

    #[tool(
        description = "Fetch multiple URLs and convert each into clean, agent-ready Markdown \
        in a single call (at most 20; fetched concurrently). Returns one result block per URL, \
        in input order, each prefixed with an HTML comment naming the URL and status \
        (`<!-- distill url=\"...\" status=\"ok|error\" -->`); a failing URL yields an error \
        block rather than failing the whole batch. Same SSRF guard and options as distill_url."
    )]
    async fn distill_urls(
        &self,
        Parameters(p): Parameters<UrlsParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.urls.is_empty() {
            return Err(McpError::invalid_params("`urls` must not be empty", None));
        }
        if p.urls.len() > MAX_BATCH {
            return Err(McpError::invalid_params(
                format!("too many URLs: {} (max {MAX_BATCH})", p.urls.len()),
                None,
            ));
        }

        let render = parse_render(p.render.as_deref())?;
        let opts = build_options(&p.options, render)?;
        let frontmatter = opts.frontmatter;
        let agent_ready = p.options.agent_ready.unwrap_or(false);

        // Spawn every URL up front; a semaphore caps how many fetch at once.
        let sem = Arc::new(Semaphore::new(BATCH_CONCURRENCY));
        let mut handles = Vec::with_capacity(p.urls.len());
        for url in p.urls {
            let sem = sem.clone();
            let opts = opts.clone();
            handles.push(tokio::spawn(async move {
                // Hold the permit for the whole fetch to bound concurrency.
                let _permit = sem.acquire_owned().await.ok();
                let u = url.clone();
                let res = tokio::task::spawn_blocking(move || {
                    crate::distill_url(&u, opts).map(|doc| {
                        if agent_ready {
                            doc.render_agent_ready()
                        } else {
                            doc.render(frontmatter)
                        }
                    })
                })
                .await;
                (url, res)
            }));
        }

        // Await in input order so results line up with the request.
        let mut blocks = Vec::with_capacity(handles.len());
        for handle in handles {
            let (url, res) = handle.await.map_err(|e| {
                McpError::internal_error(format!("batch task failed: {e}"), None)
            })?;
            let block = match res {
                Ok(Ok(md)) => format!("<!-- distill url=\"{url}\" status=\"ok\" -->\n{md}"),
                Ok(Err(e)) => {
                    format!("<!-- distill url=\"{url}\" status=\"error\" -->\n{e:#}")
                }
                Err(join) => format!(
                    "<!-- distill url=\"{url}\" status=\"error\" -->\nconversion task failed: {join}"
                ),
            };
            blocks.push(ContentBlock::text(block));
        }
        Ok(CallToolResult::success(blocks))
    }

    #[tool(
        description = "Convert raw HTML (that you already have) into clean, agent-ready \
        Markdown. No network access. Pass `base` to resolve relative links/images to \
        absolute URLs."
    )]
    async fn distill_html(
        &self,
        Parameters(p): Parameters<HtmlParams>,
    ) -> Result<CallToolResult, McpError> {
        let opts = build_options(&p.options, RenderMode::Never)?;
        let frontmatter = opts.frontmatter;
        let agent_ready = p.options.agent_ready.unwrap_or(false);
        let html = p.html;

        let markdown = tokio::task::spawn_blocking(move || {
            let doc = crate::distill_html(&html, &opts);
            if agent_ready {
                doc.render_agent_ready()
            } else {
                doc.render(frontmatter)
            }
        })
        .await
        .map_err(|e| McpError::internal_error(format!("conversion task failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![ContentBlock::text(markdown)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DistillServer {
    fn get_info(&self) -> ServerInfo {
        // ServerInfo is #[non_exhaustive]; start from Default and set fields.
        let mut info = ServerInfo::default();
        // `Implementation::from_build_env()` bakes in rmcp's own crate name
        // (the `env!` expands inside rmcp), so override with distill's.
        let mut server_info = Implementation::from_build_env();
        server_info.name = env!("CARGO_PKG_NAME").to_string();
        server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info.server_info = server_info;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Convert a URL or raw HTML into clean, deterministic Markdown, or set \
            agent_ready=true for structured JSON (sectioned Markdown + RAG chunks + schema). \
            Use distill_url for a live page, distill_html for HTML you already hold."
                .into(),
        );
        info
    }
}

/// Map the render string onto a [`RenderMode`], defaulting to `auto`.
fn parse_render(s: Option<&str>) -> Result<RenderMode, McpError> {
    match s {
        None => Ok(RenderMode::Auto),
        Some(v) => RenderMode::parse(v).ok_or_else(|| {
            McpError::invalid_params(
                format!("invalid render mode '{v}' (use never|auto|always)"),
                None,
            )
        }),
    }
}

/// Translate the tool's optional knobs into a concrete [`Options`].
fn build_options(o: &ConvOptions, render: RenderMode) -> Result<Options, McpError> {
    let base_url = match &o.base {
        Some(b) => Some(
            Url::parse(b)
                .map_err(|e| McpError::invalid_params(format!("invalid base URL: {e}"), None))?,
        ),
        None => None,
    };
    Ok(Options {
        include_links: o.include_links.unwrap_or(true),
        include_images: o.include_images.unwrap_or(true),
        frontmatter: o.frontmatter.unwrap_or(true),
        raw: o.raw.unwrap_or(false),
        render,
        base_url,
    })
}

/// Run the server, serving MCP over stdio until the client disconnects.
pub async fn run() -> anyhow::Result<()> {
    use rmcp::transport::stdio;
    use rmcp::ServiceExt;

    let service = DistillServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(o: ConvOptions) -> Options {
        build_options(&o, RenderMode::Never).unwrap()
    }

    #[test]
    fn options_default_to_cli_defaults() {
        let o = opts(ConvOptions::default());
        assert!(o.include_links && o.include_images && o.frontmatter && !o.raw);
        assert!(o.base_url.is_none());
    }

    #[test]
    fn options_respect_overrides() {
        let o = opts(ConvOptions {
            include_links: Some(false),
            include_images: Some(false),
            frontmatter: Some(false),
            raw: Some(true),
            base: Some("https://example.com/docs/".into()),
            agent_ready: Some(true),
        });
        assert!(!o.include_links && !o.include_images && !o.frontmatter && o.raw);
        assert_eq!(o.base_url.unwrap().as_str(), "https://example.com/docs/");
    }

    #[test]
    fn bad_base_url_is_rejected() {
        let o = ConvOptions {
            base: Some("not a url".into()),
            ..Default::default()
        };
        assert!(build_options(&o, RenderMode::Never).is_err());
    }

    #[test]
    fn render_mode_parsing() {
        assert_eq!(parse_render(None).unwrap(), RenderMode::Auto);
        assert_eq!(parse_render(Some("always")).unwrap(), RenderMode::Always);
        assert_eq!(parse_render(Some("never")).unwrap(), RenderMode::Never);
        assert!(parse_render(Some("sometimes")).is_err());
    }

    #[tokio::test]
    async fn distill_html_tool_produces_markdown() {
        let server = DistillServer::new();
        let params = HtmlParams {
            html: "<html><head><title>Hi</title></head><body><main><h1>Title</h1>\
                   <p>Hello <a href=\"/x\">link</a></p></main></body></html>"
                .into(),
            options: ConvOptions {
                base: Some("https://example.com".into()),
                ..Default::default()
            },
        };
        let result = server.distill_html(Parameters(params)).await.unwrap();
        let text = result
            .content
            .iter()
            .find_map(|c| c.as_text().map(|t| t.text.clone()))
            .expect("text content");
        assert!(text.contains("# Title"), "got: {text}");
        assert!(text.contains("(https://example.com/x)"), "got: {text}");
        assert!(text.contains("title: Hi"), "frontmatter missing: {text}");
    }

    #[tokio::test]
    async fn distill_html_agent_ready_returns_json_layers() {
        let server = DistillServer::new();
        let params = HtmlParams {
            html: "<html><head><title>Hi</title></head><body><main>\
                   <h1>Title</h1><p>Hello <a href=\"/x\">link</a> with enough words here.</p>\
                   <h2>API</h2>\
                   <table><tr><th>Name</th><th>Type</th></tr>\
                   <tr><td>id</td><td>string</td></tr></table>\
                   </main></body></html>"
                .into(),
            options: ConvOptions {
                base: Some("https://example.com".into()),
                agent_ready: Some(true),
                ..Default::default()
            },
        };
        let result = server.distill_html(Parameters(params)).await.unwrap();
        let text = result
            .content
            .iter()
            .find_map(|c| c.as_text().map(|t| t.text.clone()))
            .expect("text content");
        assert!(text.contains("\"sectioned_markdown\""), "got: {text}");
        assert!(text.contains("\"chunks\""), "got: {text}");
        assert!(text.contains("\"schema\""), "got: {text}");
        assert!(text.contains("https://example.com/x"), "got: {text}");
        assert!(text.contains("chunk-"), "got: {text}");
    }

    #[tokio::test]
    async fn distill_urls_rejects_empty_and_oversized() {
        let server = DistillServer::new();

        let empty = UrlsParams {
            urls: vec![],
            render: None,
            options: ConvOptions::default(),
        };
        assert!(server.distill_urls(Parameters(empty)).await.is_err());

        let oversized = UrlsParams {
            urls: (0..MAX_BATCH + 1)
                .map(|i| format!("https://example.com/{i}"))
                .collect(),
            render: None,
            options: ConvOptions::default(),
        };
        assert!(server.distill_urls(Parameters(oversized)).await.is_err());
    }

    #[tokio::test]
    async fn distill_urls_reports_per_url_errors_in_order() {
        let server = DistillServer::new();
        // Both are loopback: rejected by the SSRF guard, no network touched.
        let params = UrlsParams {
            urls: vec![
                "http://127.0.0.1/a".into(),
                "http://127.0.0.1/b".into(),
            ],
            render: None,
            options: ConvOptions::default(),
        };
        let result = server.distill_urls(Parameters(params)).await.unwrap();
        assert_eq!(result.content.len(), 2);
        let texts: Vec<String> = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect();
        assert!(texts[0].contains("url=\"http://127.0.0.1/a\""), "{:?}", texts);
        assert!(texts[1].contains("url=\"http://127.0.0.1/b\""), "{:?}", texts);
        assert!(texts.iter().all(|t| t.contains("status=\"error\"")), "{:?}", texts);
    }

    #[tokio::test]
    async fn distill_url_blocks_private_address() {
        let server = DistillServer::new();
        let params = UrlParams {
            url: "http://127.0.0.1:8080/admin".into(),
            render: None,
            options: ConvOptions::default(),
        };
        // No network is touched: the SSRF guard rejects the IP literal first.
        let err = server.distill_url(Parameters(params)).await.unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("ssrf")
                || err.message.to_lowercase().contains("non-public"),
            "expected SSRF rejection, got: {err:?}"
        );
    }
}
