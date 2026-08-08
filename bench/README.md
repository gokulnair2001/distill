# distill benchmark harness

Scores `distill` against cloud competitors (**Jina Reader**, **Firecrawl**) and
local/offline alternatives (**trafilatura**, **readability-lxml + html2text**,
**markitdown**) over a bucketed URL corpus, across five layers. Its job is to
turn "the output looks good" into defensible numbers — and to expose where
distill loses.

## Quick start

```bash
cd bench
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
cargo build --release --manifest-path ../Cargo.toml   # needed for the distill runner

./.venv/bin/python bench.py --tools distill,jina,trafilatura,readability,markitdown
```

`--tools` accepts any subset of `distill,jina,firecrawl,trafilatura,readability,markitdown`
(default is `distill,jina,firecrawl`). A tool that's missing a binary/key/package
is skipped with a one-line reason instead of failing the run — safe to always
pass the full list.

Results print as a scorecard and are written to `results/` (`results.json`,
`scorecard.md`, and per-tool raw Markdown under `results/raw/`).

## Layers

| Layer | Metric | Needs |
|---|---|---|
| `basics` | coverage %, speed (ms), determinism | nothing |
| `structural` | table / code / link / heading / image preservation vs source | nothing |
| `efficiency` | token cost (tiktoken if installed, else chars/4) | nothing |
| `content` | token P/R/F1, ROUGE-L, bloat ratio vs **gold** | gold files |
| `qa` | ⭐ agent answer accuracy from each tool's output | `ANTHROPIC_API_KEY` |

Run a subset: `--layers basics,structural`. Limit URLs: `--limit 3`.

## Alternatives covered

| Tool | Type | Notes |
|---|---|---|
| `jina` | Cloud API | r.jina.ai, renders JS, no key needed (rate-limited) |
| `firecrawl` | Cloud API | Needs `FIRECRAWL_API_KEY` |
| `trafilatura` | Local, offline | Popular Python extraction library |
| `readability` | Local, offline | readability-lxml + html2text — the classic "reader mode" pipeline many tools wrap |
| `markitdown` | Local, offline | Microsoft's HTML/doc→Markdown tool; closest direct competitor (same "agent-ready Markdown" goal, no boilerplate stripping) |

Install local alternatives via `requirements.txt` (already includes them).
Run all of them together: `./.venv/bin/python bench.py --tools distill,jina,trafilatura,readability,markitdown`.

## Enabling the gated pieces

- **Firecrawl**: `export FIRECRAWL_API_KEY=...` then add `firecrawl` to `--tools`.
- **Jina** (optional, higher rate limit): `export JINA_API_KEY=...`.
- **Content F1**: drop plain-text gold files at `corpus/gold/<id>.txt` (one per
  corpus id). Only ids with gold are scored. Zyte's
  [article-extraction-benchmark](https://github.com/zytedata/article-extraction-benchmark)
  is a good source of labeled article gold.
- **Agent QA** (north star): `export ANTHROPIC_API_KEY=...`
  (optionally `DISTILL_BENCH_MODEL`, default `claude-sonnet-5`).

## Corpus

`corpus.jsonl` — one JSON object per line: `{id, bucket, url}`. 50 URLs
across six buckets, deliberately weighted toward incumbent failure modes:
`article`, `docs`, `product`, `forum`, `table`, `spa`. Add your own URLs
freely.

## Interpreting results (important)

- **Coverage % is a weak signal.** A tool can return a small fragment and still
  "pass". Under-extraction hides here — cross-check with `content` / `qa`.
- **Token count is double-edged.** Fewer tokens = cheaper *or* = dropped content.
  Only gold-F1 / agent-QA tell you which.
- **Structural preservation counts boilerplate.** `links`/`headings` include nav
  junk, so a higher ratio can just mean "kept more noise". `tables`/`code_blocks`
  are the higher-signal features.
- **Same-input fairness**: determinism runs distill twice on identical cached
  HTML so page volatility doesn't look like nondeterminism. Cloud tools fetch
  live, so their speed includes network + render (that's the real product cost).
- **Two speed numbers.** `e2e_ms` is end-to-end (fetch + process), single trial,
  and dominated by network variance — treat it as indicative only. `proc_ms` is
  distill's process-only time on cached HTML, median of `--trials` (default 5) —
  the stable, network-free speed. Cloud tools can't separate the two, so their
  `proc_ms` is `-`.
- **Structural = MICRO-average** `Σkept/Σsource` with `[Σkept/Σsource n=pages]`
  shown, so tiny samples (code/tables often n≤3) aren't mistaken for solid
  averages. A ratio >1.0 (e.g. Jina links/images) means the tool emitted more
  than the content-scoped source had — i.e. boilerplate inflation, not fidelity.

## Known findings (distill vs 4 alternatives, 50 URLs, basics+structural+efficiency)

This corpus grew from an initial 9 URLs to 50 across the same six buckets,
specifically to move past small-sample noise and get a defensible read on
distill vs the field. Along the way it caught three real product bugs and
three benchmark ground-truth bugs — full list, in the order found:

**Product bugs (fixed):**

1. **`src/extract.rs`** — sibling-merging only looked one DOM level up, so
   content split across sibling wrapper `<div>`s (e.g. MDN's
   `layout__header`/`layout__body` split) was stranded outside the merged
   container and silently dropped. Fixed by climbing ancestors while the
   merged candidate captures too little of the page's total scored content
   (`COVERAGE_THRESHOLD`, capped by `MAX_CLIMBS`).
2. **`src/convert.rs`** — `render_dl` always rendered `<dd>` content as a
   single inline run, which destroyed nested `<pre>` code blocks,
   paragraphs, and lists inside Sphinx-style API reference entries (e.g.
   Python's own docs put full method examples inside `<dd>`). Fixed by
   rendering `<dd>` bodies as blocks.
3. **`src/lib.rs`** — `needs_render`'s auto-render heuristic only checked
   raw body word count and a fixed list of known SPA mount points
   (`#root`, `#__next`, etc). Nav-heavy marketing pages have plenty of body
   text that's all chrome (e.g. redis.io: 946 words, real content ~80
   chars), and modern Next.js App Router pages don't render `#__next` at
   all, so both checks missed real under-rendered pages. Fixed by running
   the actual extraction pipeline as the trigger signal instead of
   guessing from word counts — caught and fixed a real under-extraction
   case (`redis-home`: 150 chars → 7,390 chars). One related, *unfixed*
   limitation: a small number of heavy client-rendered docs sites
   (`terraform-syntax-docs`) still don't finish populating within the
   current 8s headless-Chrome render budget — a timing/completeness issue,
   not a detection issue.

**Benchmark bugs (fixed, not product issues):**

4. `_TABLE_SEP` in `metrics.py` matched bare `---` lines without a leading
   `|`, which accidentally counted trafilatura's YAML frontmatter
   delimiters as tables (its runner is the only one built with
   `with_metadata=True`). Inflated its table-fidelity score to 0.95 —
   fixed by requiring the leading pipe real GFM tables use.
5. `expected_features()` counted every `<table>` in `<body>`, including
   MediaWiki `navbox`-classed and `role="presentation"` tables — pure
   navigation chrome that just isn't wrapped in a semantic `<nav>` tag.
   That penalized every tool for correctly *not* extracting boilerplate,
   and let jina's fabricated pseudo-tables (it converts some navbox
   link-lists into single-column Markdown tables that don't correspond to
   any real source `<table>`) count as wins. Fixed by excluding both.
6. `expected_features()` also counted `<pre>` blocks nested inside table
   cells as "expected" code blocks — but a GFM table cell is one line, so
   no tool can represent that as a real fenced block without breaking the
   table. Fixed by excluding table-nested `<pre>`.

Current numbers (50 URLs, all fixes applied):

| tool | coverage | avg tokens | table fidelity | code-block fidelity | local? |
|---|---|---|---|---|---|
| **distill** | 92% | 15,251 | 0.62 | **0.94** | yes |
| jina | 100%* | 23,282 | 0.27 | 0.17 | no (cloud) |
| trafilatura | 92% | 14,610 | 0.18 | 0.69 | yes |
| readability | 80% | 8,417 | 0.00 | 0.00 | yes |
| markitdown | 66% | 12,690 | 0.57 | 0.68 | yes |

\* jina's coverage is real, but its links/headings/images ratios all sit at
0.9-1.13 — it's keeping boilerplate its struct mean rewards; on the two
high-signal features (tables, code) it's the weakest tool of the five.

- **At this scale, distill leads decisively on both high-signal structural
  features** — no longer a "matches once artifacts are removed" result
  like the 9-URL corpus, but a clear lead against every alternative,
  local or cloud.
- distill's own coverage (92%) has four real failures, none of them
  extraction bugs: one dead URL (404), two StackOverflow bot-blocks (403),
  and the one known render-budget limitation above.
- readability (the classic reader-mode pipeline) remains the weakest
  overall: worst link/heading retention and real under-extraction on some
  pages, not genuine efficiency.
- Full per-page numbers: `results/scorecard.md` and `results/results.json`.
  Content-quality (gold-F1) and agent-QA layers still need gold files /
  `ANTHROPIC_API_KEY` to run — open question until then.
