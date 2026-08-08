# distill

Convert any website into clean, **agent-ready Markdown**. Local-first, fast, deterministic.

`distill` is a CLI that turns a URL (or raw HTML) into Markdown an LLM agent can
actually use: boilerplate stripped, structure preserved (tables, code blocks,
nested lists), links resolved to absolute URLs, and metadata in YAML frontmatter.
No cloud, no per-page cost, byte-identical output for the same input.

## Why

Most extractors are tuned for *articles* and fall apart on the pages agents
actually hit — docs, tables, product/pricing pages, SPAs. `distill` treats
structural fidelity and non-article content as first-class, and runs entirely on
your machine.

| | distill | Jina Reader | Firecrawl | trafilatura |
|---|---|---|---|---|
| Local / no cost | ✅ | ❌ cloud | ❌ cloud/paid | ✅ |
| Deterministic output | ✅ | ⚠️ | ⚠️ | ✅ |
| Tables → real MD tables | ✅ | ⚠️ | ⚠️ | ❌ |
| Code blocks + language | ✅ | ⚠️ | ✅ | ❌ |
| Absolute link resolution | ✅ | ⚠️ | ✅ | ⚠️ |
| JS / SPA rendering | 🚧 planned | ✅ | ✅ | ❌ |

## Install

```bash
cargo build --release
# binary at ./target/release/distill
```

## Usage

```bash
# From a URL
distill https://en.wikipedia.org/wiki/Markdown

# From a local HTML file
distill page.html

# From stdin
curl -s https://example.com | distill -

# Options
distill <url> \
  --no-frontmatter \   # omit the YAML metadata block
  --no-links \         # keep link text only
  --no-images \        # drop images
  --raw \              # skip main-content extraction (convert whole page)
  --base <url> \       # base for resolving relative links
  -o out.md            # write to a file
```

## How it works

```
URL → fetch → metadata → clean → extract → convert → Markdown
```

1. **fetch** — browser-like headers, gzip/brotli, redirect + encoding handling.
2. **metadata** — title / author / date / lang / canonical from `<head>`.
3. **clean** — strip scripts, chrome (nav/footer/aside), and boilerplate matched
   by id/class (word-boundaried so `thread`/`download` survive), plus hidden nodes.
4. **extract** — Readability-style scoring: text blocks pass points to their
   parent and grandparent; the link-density-adjusted winner is the main content.
5. **convert** — DOM→Markdown with real tables, fenced code + language, resolved
   absolute links, nested lists, blockquotes.

## Performance

~10 ms to parse + clean + extract + convert a 300 KB page (Wikipedia), local,
no network round-trip.

## Roadmap

- [ ] **JS rendering** — static-first, headless-Chromium fallback only when the
      DOM looks under-rendered (SPA coverage without paying browser cost per page).
- [ ] **Page-type awareness** — distinct strategies for docs / listings / tables.
- [ ] **MCP server** — expose `distill` as a tool any local agent can call.
- [ ] **Structured extraction** — schema-guided JSON output + RAG chunking.
- [ ] **Benchmark harness** — scored corpus vs Jina / Firecrawl / trafilatura.

## Development

```bash
cargo test          # unit + integration tests
cargo build --release
```

## License

MIT
