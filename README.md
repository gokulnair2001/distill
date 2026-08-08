# distill

Convert any website into clean, **agent-ready Markdown**. Local-first, fast, deterministic.

`distill` is a CLI that turns a URL (or raw HTML) into Markdown an LLM agent can
actually use: boilerplate stripped, structure preserved (tables, code blocks,
nested lists), links resolved to absolute URLs, and metadata in YAML frontmatter.
No cloud, no per-page cost, byte-identical output for the same input.

Use it as a **CLI** or run it as an **MCP server** so agents can call it as a
tool (`distill_url`, `distill_urls`, `distill_html`).

## Why

Agents mostly need to read docs, tables, and product/pricing pages — not just
clean articles. `distill` is built for that: structural fidelity and non-article
content are first-class, and it runs entirely on your machine.

**What you get**

- **Local & private** — no cloud, no per-page cost, nothing leaves your machine.
- **Deterministic** — byte-identical Markdown for the same input.
- **Structure-preserving** — real Markdown tables, fenced code with language,
  nested lists, and blockquotes.
- **Absolute links** — relative URLs and images resolved against the page.
- **Metadata** — title / author / date / canonical in YAML frontmatter.
- **JS / SPA aware** — optional headless-Chrome fallback for client-rendered pages.

## Install

Published as **`distill-md`** (the `distill` name is taken on crates.io/npm);
the installed command is still `distill`.

```bash
# Agents (MCP server) — no install, always latest
npx -y -p distill-md distill-mcp

# Shell installer (macOS / Linux) — prebuilt, no toolchain
curl -LsSf https://github.com/gokulnair2001/distill/releases/latest/download/distill-md-installer.sh | sh

# Rust toolchain
cargo binstall distill-md      # prebuilt
cargo install distill-md       # from source

# From this repo
cargo build --release --features mcp   # binaries at ./target/release/{distill,distill-mcp}
```

Prebuilt binaries for macOS (arm64/x64), Linux (arm64/x64), and Windows (x64)
are attached to every [GitHub Release](https://github.com/gokulnair2001/distill/releases).
See [RELEASING.md](RELEASING.md) for how releases are produced.

## Usage

```bash
# From a URL
distill https://en.wikipedia.org/wiki/Markdown

# From a local HTML file
distill page.html

# From stdin
curl -s https://example.com | distill -

# Multiple inputs → one file per input (‑o is treated as a directory).
# A failing input is reported and skipped; the batch still writes the rest.
distill https://a.com https://b.com page.html -o out/
#   → out/a.com.md, out/b.com.md, out/page.md

# Options
distill <url> \
  --render auto \      # JS rendering: never | auto | always (default: auto)
  --no-frontmatter \   # omit the YAML metadata block
  --no-links \         # keep link text only
  --no-images \        # drop images
  --raw \              # skip main-content extraction (convert whole page)
  --base <url> \       # base for resolving relative links
  -o out.md            # write to a file (or a directory, for multiple inputs)
```

### JavaScript rendering

Client-rendered pages (SPAs whose content is injected by JS) return an empty
shell over plain HTTP. `distill` handles this **static-first**: it fetches
statically and, in `--render auto`, only spins up a headless browser
when the page looks under-rendered (empty `#root`/`#app`, near-empty body).
`--render always` forces it; `--render never` disables it.

Requires an installed Chrome/Chromium/Brave/Edge; set `DISTILL_CHROME` to point
at a specific binary. If none is found, distill falls back to the static HTML.

> Note: statically pre-rendered sites (Next.js/VitePress SSG, most docs sites)
> already contain their content, so rendering them changes nothing — it only
> helps genuinely client-rendered pages.

## Use as an MCP server

`distill` ships an [MCP](https://modelcontextprotocol.io) server so any local
agent (Claude Code, etc.) can call it as a tool instead of shelling out. It
speaks the protocol over stdio and exposes three tools:

- **`distill_url`** — fetch a URL and convert it. SSRF-guarded (see below).
- **`distill_urls`** — fetch and convert up to 20 URLs in one call (fetched 4 at
  a time). Returns one result block per URL, in input order, each prefixed with
  `<!-- distill url="..." status="ok|error" -->`; a failing URL yields an error
  block instead of failing the whole batch.
- **`distill_html`** — convert HTML you already have. No network.

All accept the same knobs as the CLI (`include_links`, `include_images`,
`frontmatter`, `raw`, `base`; `distill_url`/`distill_urls` also take `render`).

Build the server binary (it's behind a feature flag so the plain CLI stays lean):

```bash
cargo build --release --features mcp
# binary at ./target/release/distill-mcp
```

Register it with Claude Code — zero-install via npx (once published), or a
local build:

```bash
# Published: no install, always latest
claude mcp add distill -- npx -y -p distill-md distill-mcp

# Local build
claude mcp add distill -- /absolute/path/to/target/release/distill-mcp
```

Or add it to an `mcp.json` yourself:

```json
{
  "mcpServers": {
    "distill": {
      "command": "npx",
      "args": ["-y", "-p", "distill-md", "distill-mcp"]
    }
  }
}
```

(`-p distill-md distill-mcp` selects the `distill-mcp` binary from the
`distill-md` package, which also provides the `distill` CLI.)

### SSRF guard

Because `distill_url` will fetch whatever URL an agent hands it, requests to
non-public addresses — loopback, private ranges, link-local (including the
`169.254.169.254` cloud-metadata endpoint), and their IPv6 equivalents — are
**refused by default**, on the initial URL and on every redirect/`meta refresh`
hop. To distill a local dev server or an internal host, opt out explicitly:

```bash
DISTILL_ALLOW_PRIVATE_HOSTS=1 distill http://localhost:3000
```

The same variable governs the CLI and the MCP server.

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

## Benchmarks

Numbers below are from a 50-URL corpus (`bench/corpus.jsonl`) spanning
articles, docs, product pages, forum threads, table-heavy pages, and SPAs,
measured on 2026-08-09 against the current build, head-to-head with **Jina
Reader**, **Firecrawl**, **trafilatura**, **readability-lxml**, and
**markitdown**. Reproduce with the harness in [bench/](bench/README.md).

| metric | distill |
|---|---|
| pages | 50 |
| coverage | 92% returned usable output |
| process time (median, network-free) | ~16 ms/page |
| output size | ~15.3k tokens/page (avg) |
| deterministic | yes — byte-identical on re-run |

**Structural preservation** — micro-average `Σkept / Σsource` (higher isn't
always better; `links`/`headings` include page boilerplate), distill vs. the
field:

| tool | tables | code blocks | coverage | local? |
|---|---|---|---|---|
| **distill** | 0.62 | **0.94** | 92% | yes |
| jina | 0.27 | 0.17 | 100%* | no (cloud) |
| trafilatura | 0.18 | 0.69 | 92% | yes |
| readability | 0.00 | 0.00 | 80% | yes |
| markitdown | 0.57 | 0.68 | 66% | yes |

\* jina's coverage is real, but its higher struct mean is inflated by
boilerplate (links/headings/images all ≥0.9-1.0, meaning it keeps nav
junk it shouldn't) — on the two high-signal features that actually
indicate content quality, it's the weakest of the group.

Honest read: distill leads decisively on both high-signal structural
features (tables, code blocks) against every alternative, local or cloud,
at this larger and more representative scale. This benchmark also caught a
real bug: distill's `--render auto` heuristic missed modern marketing pages
built from short, scattered content blocks (plenty of raw body text, but
none of it substantial) — fixed by having the heuristic run the actual
extraction pipeline as its own trigger signal instead of guessing from word
counts. One known limitation remains: a small number of heavy client-rendered
docs sites don't finish populating within the current headless-Chrome render
budget. Full history of what was found and fixed — including two earlier
extraction/conversion bugs and three benchmark ground-truth bugs discovered
along the way — is in [bench/README.md](bench/README.md). End-to-end timing
is network-dominated and only indicative; the process-only median is the
stable figure.

## Roadmap

- [x] **JS rendering** — static-first, headless-Chrome fallback only when the
      DOM looks under-rendered (SPA coverage without paying browser cost per page).
- [x] **Benchmark harness** — scored corpus vs Jina / Firecrawl (see `bench/`).
- [ ] **Page-type awareness** — distinct strategies for docs / listings / tables.
- [ ] **Structural fidelity** — definition lists, table colspan/rowspan, inline
      code backtick escaping, `<picture>`/`srcset`.
- [x] **MCP server** — expose `distill` as a tool any local agent can call
      (`distill_url` / `distill_urls` / `distill_html`, SSRF-guarded).
      See "Use as an MCP server".
- [ ] **Structured extraction** — schema-guided JSON output + RAG chunking.

## Development

```bash
cargo test                     # unit + integration tests
cargo test --features mcp      # include the MCP server
cargo build --release
```

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
