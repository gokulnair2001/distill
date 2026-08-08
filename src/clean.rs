use kuchikiki::NodeRef;
use once_cell::sync::Lazy;
use regex::Regex;

/// Tags that never carry main content and should be dropped outright.
const DROP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "iframe", "svg", "canvas", "form",
    "button", "input", "select", "textarea", "nav", "footer", "aside", "header",
    "menu", "dialog",
];

/// Boilerplate identifiers matched against an element's `id` + `class`.
/// Word-boundaried to avoid nuking things like "thread" (contains "read") or
/// "download" (contains "ad").
static BOILERPLATE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(^|[-_ ])(ad|ads|advert|advertisement|banner|breadcrumbs?|byline|comment|comments|cookie|consent|disqus|footer|gdpr|masthead|menu|nav|navbar|navigation|newsletter|popup|promo|related|share|sharing|sidebar|social|sponsor|subscribe|toolbar|widget|modal|overlay|cta|pagination|pager|skip-link|screen-reader)([-_ ]|s?$)",
    )
    .unwrap()
});

/// Strip scripts, chrome, and boilerplate from the DOM in place.
/// Returns the (possibly cleaned) root so callers can chain.
pub fn clean(root: &NodeRef) {
    let dbg = std::env::var("DISTILL_DEBUG").is_ok();
    macro_rules! trace {
        ($label:expr) => {
            if dbg {
                let n = root
                    .select_first("body")
                    .map(|b| b.text_contents().chars().count())
                    .unwrap_or(0);
                eprintln!("[debug] after {}: body chars = {}", $label, n);
            }
        };
    }

    // 1. Drop unwanted tags.
    drop_by_tag(root);
    trace!("drop_by_tag");

    // 2. Drop elements whose id/class looks like boilerplate.
    drop_by_identifier(root);
    trace!("drop_by_identifier");

    // 3. Drop hidden elements.
    drop_hidden(root);
    trace!("drop_hidden");

    // 4. Remove HTML comments.
    drop_comments(root);
    trace!("drop_comments");
}

fn drop_by_tag(root: &NodeRef) {
    for tag in DROP_TAGS {
        let matches: Vec<_> = root
            .select(tag)
            .map(|s| s.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for m in matches {
            // `<header>` is page chrome at the top level, but inside <article>/
            // <main> it usually holds the real title/byline — keep those.
            if *tag == "header" && ancestor_is(m.as_node(), &["article", "main"]) {
                continue;
            }
            m.as_node().detach();
        }
    }
}

/// Does `node` have an ancestor element whose tag is in `names`?
fn ancestor_is(node: &NodeRef, names: &[&str]) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if let Some(el) = n.as_element() {
            if names.contains(&el.name.local.as_ref()) {
                return true;
            }
        }
        cur = n.parent();
    }
    false
}

fn drop_by_identifier(root: &NodeRef) {
    // Collect first, then detach (don't mutate while iterating the tree).
    let mut to_remove = Vec::new();
    for el in root.select("*").unwrap() {
        let node = el.as_node();
        // Never strip structural roots by id/class — sites like Wikipedia put
        // classes such as `vector-feature-main-menu-...` on <html>, which would
        // otherwise nuke the entire document.
        if matches!(el.name.local.as_ref(), "html" | "body" | "main" | "article") {
            continue;
        }
        let attrs = el.attributes.borrow();
        let id = attrs.get("id").unwrap_or("");
        let class = attrs.get("class").unwrap_or("");
        let role = attrs.get("role").unwrap_or("");
        if role == "navigation" || role == "banner" || role == "complementary" {
            to_remove.push(node.clone());
            continue;
        }
        if (!id.is_empty() && BOILERPLATE.is_match(id))
            || (!class.is_empty() && BOILERPLATE.is_match(class))
        {
            to_remove.push(node.clone());
        }
    }
    for n in to_remove {
        n.detach();
    }
}

fn drop_hidden(root: &NodeRef) {
    let mut to_remove = Vec::new();
    for el in root.select("*").unwrap() {
        let attrs = el.attributes.borrow();
        let hidden = attrs.get("hidden").is_some()
            || attrs.get("aria-hidden").map(|v| v == "true").unwrap_or(false)
            || attrs
                .get("style")
                .map(|s| {
                    let s = s.replace(' ', "").to_lowercase();
                    s.contains("display:none") || s.contains("visibility:hidden")
                })
                .unwrap_or(false);
        if hidden {
            to_remove.push(el.as_node().clone());
        }
    }
    for n in to_remove {
        n.detach();
    }
}

fn drop_comments(root: &NodeRef) {
    let mut to_remove = Vec::new();
    for node in root.inclusive_descendants() {
        if node.as_comment().is_some() {
            to_remove.push(node);
        }
    }
    for n in to_remove {
        n.detach();
    }
}
