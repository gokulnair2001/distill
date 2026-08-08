use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;
use url::Url;

/// A fetched page: the HTML body and the *final* URL after redirects
/// (used as the base for resolving relative links).
pub struct Fetched {
    pub html: String,
    pub final_url: Url,
}

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// `<meta http-equiv="refresh" content="0; url=...">` in either attribute order.
static META_REFRESH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?is)<meta[^>]*?http-equiv\s*=\s*["']?refresh["']?[^>]*?content\s*=\s*["']([^"']*)["']"#,
    )
    .unwrap()
});
static META_REFRESH_REV: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?is)<meta[^>]*?content\s*=\s*["']([^"']*)["'][^>]*?http-equiv\s*=\s*["']?refresh["']?"#,
    )
    .unwrap()
});
static URL_IN_CONTENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)url\s*=\s*['"]?([^'";]+)"#).unwrap());

/// Fetch a URL, following HTTP redirects *and* client-side `<meta refresh>`
/// redirects (up to a small hop limit). Returns the final HTML and URL.
pub fn fetch(url: &str) -> Result<Fetched> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("building HTTP client")?;

    let mut current = Url::parse(url).with_context(|| format!("invalid URL: {url}"))?;
    let mut visited: Vec<String> = Vec::new();

    for _ in 0..4 {
        let resp = client
            .get(current.clone())
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .with_context(|| format!("fetching {current}"))?;

        let status = resp.status();
        let final_url = resp.url().clone();
        if !status.is_success() {
            anyhow::bail!("HTTP {} for {}", status.as_u16(), current);
        }
        let html = resp.text().context("reading response body")?;
        visited.push(final_url.to_string());

        // Follow a client-side meta-refresh redirect if it points somewhere new.
        if let Some(target) = meta_refresh_target(&html, &final_url) {
            let t = target.to_string();
            if !visited.contains(&t) {
                current = target;
                continue;
            }
        }
        return Ok(Fetched { html, final_url });
    }

    // Hop limit hit: fetch the last target one more time and return it.
    let resp = client.get(current.clone()).send()?;
    let final_url = resp.url().clone();
    let html = resp.text()?;
    Ok(Fetched { html, final_url })
}

/// Extract and resolve a `<meta refresh>` target URL, if present.
fn meta_refresh_target(html: &str, base: &Url) -> Option<Url> {
    // Only scan the head-ish prefix; refresh metas live early. Cheap + avoids
    // matching stray text deep in the body.
    let head = &html[..html.len().min(8192)];
    let content = META_REFRESH
        .captures(head)
        .or_else(|| META_REFRESH_REV.captures(head))?
        .get(1)?
        .as_str();
    let raw = URL_IN_CONTENT.captures(content)?.get(1)?.as_str().trim();
    if raw.is_empty() {
        return None;
    }
    base.join(raw).ok()
}
