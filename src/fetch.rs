use anyhow::{Context, Result};
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

/// Fetch a URL with browser-like headers, decompression, and a sane timeout.
pub fn fetch(url: &str) -> Result<Fetched> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("building HTTP client")?;

    let resp = client
        .get(url)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .with_context(|| format!("fetching {url}"))?;

    let status = resp.status();
    let final_url = resp.url().clone();
    if !status.is_success() {
        anyhow::bail!("HTTP {} for {}", status.as_u16(), url);
    }

    // reqwest's .text() decodes using the charset from the Content-Type header,
    // falling back to UTF-8. Good enough for v1; meta-charset sniffing is a TODO.
    let html = resp.text().context("reading response body")?;
    Ok(Fetched { html, final_url })
}
