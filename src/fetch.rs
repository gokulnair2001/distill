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
        let ct_charset = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(charset_from_content_type);
        let bytes = resp.bytes().context("reading response body")?;
        let html = decode_html(&bytes, ct_charset.as_deref());
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
    let ct_charset = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(charset_from_content_type);
    let bytes = resp.bytes()?;
    let html = decode_html(&bytes, ct_charset.as_deref());
    Ok(Fetched { html, final_url })
}

/// Extract the `charset=` label from a Content-Type header value.
fn charset_from_content_type(ct: &str) -> Option<String> {
    let lower = ct.to_ascii_lowercase();
    let idx = lower.find("charset=")?;
    let raw = ct[idx + "charset=".len()..]
        .trim()
        .trim_matches(|c| c == '"' || c == '\'');
    let label = raw.split(|c| c == ';' || c == ' ').next()?.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// Decode HTML bytes to a String using, in priority order: the HTTP charset,
/// the `<meta charset>` declared in the document, else UTF-8. Prevents mojibake
/// on pages served as legacy encodings (Shift-JIS, Windows-1251, …).
fn decode_html(bytes: &[u8], http_charset: Option<&str>) -> String {
    let enc = http_charset
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .or_else(|| sniff_meta_charset(bytes))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = enc.decode(bytes);
    text.into_owned()
}

/// Scan the first 4 KB for a `<meta charset=...>` / `content="...; charset=..."`.
fn sniff_meta_charset(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let prefix = &bytes[..bytes.len().min(4096)];
    // Interpret as ASCII-ish for the scan; charset labels are ASCII.
    let text = String::from_utf8_lossy(prefix);
    let lower = text.to_ascii_lowercase();
    let idx = lower.find("charset=")?;
    let rest = &text[idx + "charset=".len()..];
    let label: String = rest
        .trim_start_matches(|c| c == '"' || c == '\'' || c == ' ')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    encoding_rs::Encoding::for_label(label.as_bytes())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_charset_from_content_type() {
        assert_eq!(
            charset_from_content_type("text/html; charset=UTF-8").as_deref(),
            Some("UTF-8")
        );
        assert_eq!(
            charset_from_content_type("text/html;charset=shift_jis").as_deref(),
            Some("shift_jis")
        );
        assert_eq!(charset_from_content_type("text/html").as_deref(), None);
    }

    #[test]
    fn decodes_declared_meta_charset() {
        // 0xE9 is `é` in windows-1252 but invalid standalone UTF-8.
        let bytes = b"<html><head><meta charset=windows-1252></head><body>caf\xe9</body></html>";
        let html = decode_html(bytes, None);
        assert!(html.contains("caf\u{e9}"), "got: {html}");
    }

    #[test]
    fn http_charset_wins_over_meta() {
        let bytes = b"<html><head><meta charset=utf-8></head><body>caf\xe9</body></html>";
        let html = decode_html(bytes, Some("windows-1252"));
        assert!(html.contains("caf\u{e9}"), "got: {html}");
    }
}
