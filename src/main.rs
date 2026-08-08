use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;

use distill::types::{Options, RenderMode};
use distill::{distill_html, distill_url, parse_base};

/// distill — convert any website into clean, agent-ready Markdown.
#[derive(Parser, Debug)]
#[command(name = "distill", version, about, long_about = None)]
struct Cli {
    /// URL to fetch, a local HTML file path, or `-` to read HTML from stdin.
    input: String,

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

    /// Write output to a file instead of stdout.
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

    let doc = if cli.input == "-" {
        let mut html = String::new();
        std::io::stdin().read_to_string(&mut html)?;
        distill_html(&html, &opts)
    } else if is_url(&cli.input) {
        distill_url(&cli.input, opts.clone())?
    } else if Path::new(&cli.input).exists() {
        let html = std::fs::read_to_string(&cli.input)
            .with_context(|| format!("reading {}", cli.input))?;
        distill_html(&html, &opts)
    } else {
        anyhow::bail!(
            "input is not a URL, an existing file, or '-': {}",
            cli.input
        );
    };

    let rendered = doc.render(opts.frontmatter);

    match &cli.output {
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

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}
