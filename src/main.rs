use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use url::Url;

use distill::types::{Document, Options, RenderMode};
use distill::{distill_html, distill_url, parse_base};

/// distill — convert any website into clean, agent-ready Markdown.
#[derive(Parser, Debug)]
#[command(name = "distill", version, about, long_about = None)]
struct Cli {
    /// One or more URLs, local HTML file paths, or `-` to read HTML from stdin.
    /// With several inputs, `--output` must be a directory (one file per input).
    #[arg(required = true, num_args = 1..)]
    inputs: Vec<String>,

    /// Base URL for resolving relative links (defaults to the fetched URL).
    #[arg(long)]
    base: Option<String>,

    /// Omit the YAML frontmatter metadata block.
    #[arg(long)]
    no_frontmatter: bool,

    /// Strip hyperlinks (keep only their text).
    #[arg(long)]
    no_links: bool,

    /// Strip images.
    #[arg(long)]
    no_images: bool,

    /// Skip main-content extraction; convert the whole cleaned page.
    #[arg(long)]
    raw: bool,

    /// JS rendering: never | auto | always (default: auto). Renders SPA pages
    /// with a headless browser only when needed. Ignored for file/stdin input.
    #[arg(long, default_value = "auto")]
    render: String,

    /// Output destination. For a single input this is a file (default: stdout).
    /// For multiple inputs it is a directory that receives one `<slug>.md` per input.
    #[arg(short, long)]
    output: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let render = RenderMode::parse(&cli.render)
        .with_context(|| format!("invalid --render value: {} (use never|auto|always)", cli.render))?;
    let mut opts = Options {
        include_links: !cli.no_links,
        include_images: !cli.no_images,
        frontmatter: !cli.no_frontmatter,
        raw: cli.raw,
        render,
        base_url: None,
    };
    if let Some(b) = &cli.base {
        opts.base_url = Some(parse_base(b).with_context(|| format!("invalid --base: {b}"))?);
    }

    if cli.inputs.len() == 1 {
        run_single(&cli.inputs[0], &opts, cli.output.as_deref())
    } else {
        run_batch(&cli.inputs, &opts, cli.output.as_deref())
    }
}

/// Single input: write to `-o <file>` or stdout (the original behavior).
fn run_single(input: &str, opts: &Options, output: Option<&str>) -> Result<()> {
    let doc = distill_input(input, opts)?;
    let rendered = doc.render(opts.frontmatter);
    match output {
        Some(path) => {
            std::fs::write(path, &rendered).with_context(|| format!("writing {path}"))?;
            eprintln!("wrote {} bytes to {path}", rendered.len());
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(rendered.as_bytes())?;
        }
    }
    Ok(())
}

/// Multiple inputs. With `-o <dir>` each input is written to its own
/// `<dir>/<slug>.md`; without `-o` they are concatenated to stdout with a
/// per-input delimiter. A failing input is reported but does not stop the
/// batch; the process exits non-zero if any input failed.
fn run_batch(inputs: &[String], opts: &Options, output: Option<&str>) -> Result<()> {
    let mut used = HashSet::new();
    let mut failures = 0usize;

    match output {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating output directory {dir}"))?;
            for input in inputs {
                match distill_input(input, opts) {
                    Ok(doc) => {
                        let slug = unique_slug(&slugify(input), &mut used);
                        let path = Path::new(dir).join(format!("{slug}.md"));
                        let rendered = doc.render(opts.frontmatter);
                        std::fs::write(&path, &rendered)
                            .with_context(|| format!("writing {}", path.display()))?;
                        eprintln!("wrote {} bytes to {}", rendered.len(), path.display());
                    }
                    Err(e) => {
                        eprintln!("error: {input}: {e:#}");
                        failures += 1;
                    }
                }
            }
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            for input in inputs {
                match distill_input(input, opts) {
                    Ok(doc) => {
                        writeln!(lock, "<!-- distill: {input} -->")?;
                        lock.write_all(doc.render(opts.frontmatter).as_bytes())?;
                        writeln!(lock, "\n")?;
                    }
                    Err(e) => {
                        eprintln!("error: {input}: {e:#}");
                        failures += 1;
                    }
                }
            }
        }
    }

    if failures > 0 {
        anyhow::bail!("{failures} of {} input(s) failed", inputs.len());
    }
    Ok(())
}

/// Convert one input (URL, file path, or `-` for stdin) into a [`Document`].
fn distill_input(input: &str, opts: &Options) -> Result<Document> {
    if input == "-" {
        let mut html = String::new();
        std::io::stdin().read_to_string(&mut html)?;
        Ok(distill_html(&html, opts))
    } else if is_url(input) {
        distill_url(input, opts.clone())
    } else if Path::new(input).exists() {
        let html =
            std::fs::read_to_string(input).with_context(|| format!("reading {input}"))?;
        Ok(distill_html(&html, opts))
    } else {
        anyhow::bail!("input is not a URL, an existing file, or '-': {input}")
    }
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Derive a filesystem-safe base name for an input. URLs use host + path,
/// files use their stem, stdin becomes `stdin`.
fn slugify(input: &str) -> String {
    let base = if is_url(input) {
        match Url::parse(input) {
            Ok(u) => {
                let host = u.host_str().unwrap_or("page").to_string();
                let path = u.path().trim_matches('/');
                if path.is_empty() {
                    host
                } else {
                    format!("{host}_{path}")
                }
            }
            Err(_) => input.to_string(),
        }
    } else if input == "-" {
        "stdin".to_string()
    } else {
        Path::new(input)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("page")
            .to_string()
    };
    sanitize(&base)
}

/// Reduce a string to `[A-Za-z0-9.-]`, collapsing every other run to a single
/// `_`, trimming separators, and capping the length.
fn sanitize(s: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
            out.push(c);
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    let trimmed: String = out.trim_matches(|c| c == '_' || c == '.').chars().take(100).collect();
    let trimmed = trimmed.trim_matches(|c| c == '_' || c == '.').to_string();
    if trimmed.is_empty() {
        "page".to_string()
    } else {
        trimmed
    }
}

/// Ensure a slug is unique within a batch, appending `-2`, `-3`, … on collision.
fn unique_slug(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_url_uses_host_and_path() {
        assert_eq!(
            slugify("https://en.wikipedia.org/wiki/Markdown"),
            "en.wikipedia.org_wiki_Markdown"
        );
        assert_eq!(slugify("https://example.com/"), "example.com");
        assert_eq!(slugify("https://example.com"), "example.com");
    }

    #[test]
    fn slugify_file_uses_stem() {
        assert_eq!(slugify("/tmp/some page.html"), "some_page");
        assert_eq!(slugify("-"), "stdin");
    }

    #[test]
    fn sanitize_collapses_and_trims() {
        assert_eq!(sanitize("a//b??c"), "a_b_c");
        assert_eq!(sanitize("__weird__"), "weird");
        assert_eq!(sanitize("///"), "page");
    }

    #[test]
    fn unique_slug_disambiguates_collisions() {
        let mut used = HashSet::new();
        assert_eq!(unique_slug("doc", &mut used), "doc");
        assert_eq!(unique_slug("doc", &mut used), "doc-2");
        assert_eq!(unique_slug("doc", &mut used), "doc-3");
    }
}
