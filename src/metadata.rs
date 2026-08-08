use kuchikiki::NodeRef;

use crate::types::Document;

/// Pull document-level metadata from `<head>` and semantic tags.
/// Runs on the *original* DOM (before cleanup strips things).
pub fn extract(doc: &NodeRef) -> Document {
    let mut d = Document::default();

    d.title = meta_content(doc, "property", "og:title")
        .or_else(|| meta_content(doc, "name", "twitter:title"))
        .or_else(|| text_of(doc, "title"))
        .map(|s| clean(&s))
        .filter(|s| !s.is_empty());

    d.byline = meta_content(doc, "name", "author")
        .or_else(|| meta_content(doc, "property", "article:author"))
        .or_else(|| meta_content(doc, "name", "twitter:creator"))
        .map(|s| clean(&s))
        .filter(|s| !s.is_empty());

    d.date = meta_content(doc, "property", "article:published_time")
        .or_else(|| meta_content(doc, "name", "date"))
        .or_else(|| meta_content(doc, "name", "publish-date"))
        .or_else(|| attr_of(doc, "time[datetime]", "datetime"))
        .map(|s| clean(&s))
        .filter(|s| !s.is_empty());

    d.canonical = attr_of(doc, "link[rel=canonical]", "href")
        .or_else(|| meta_content(doc, "property", "og:url"))
        .map(|s| clean(&s))
        .filter(|s| !s.is_empty());

    d.site = meta_content(doc, "property", "og:site_name")
        .map(|s| clean(&s))
        .filter(|s| !s.is_empty());

    d.lang = doc
        .select_first("html")
        .ok()
        .and_then(|h| h.attributes.borrow().get("lang").map(|s| s.to_string()))
        .map(|s| clean(&s))
        .filter(|s| !s.is_empty());

    d
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `<meta {attr}="{val}" content="...">`
fn meta_content(doc: &NodeRef, attr: &str, val: &str) -> Option<String> {
    let sel = format!("meta[{attr}=\"{val}\"]");
    let el = doc.select_first(&sel).ok()?;
    let attrs = el.attributes.borrow();
    attrs.get("content").map(|s| s.to_string())
}

fn attr_of(doc: &NodeRef, selector: &str, attr: &str) -> Option<String> {
    let el = doc.select_first(selector).ok()?;
    let attrs = el.attributes.borrow();
    attrs.get(attr).map(|s| s.to_string())
}

fn text_of(doc: &NodeRef, selector: &str) -> Option<String> {
    let el = doc.select_first(selector).ok()?;
    Some(el.text_contents())
}
