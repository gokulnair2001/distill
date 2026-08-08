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

`corpus.jsonl` — one JSON object per line: `{id, bucket, url}`. Buckets are
deliberately weighted toward incumbent failure modes: `article`, `docs`,
`product`, `forum`, `table`, `spa`. Add your own URLs freely.

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

## Known findings (distill vs 4 alternatives, 9 URLs, basics+structural+efficiency)

| tool | coverage | avg tokens | table fidelity | code-block fidelity | local? |
|---|---|---|---|---|---|
| distill | 100% | 7,715 | 0.36 | 0.41 | yes |
| jina | 100% | 15,096 | 0.77 | 0.00 | no (cloud) |
| trafilatura | 100% | 8,185 | **0.95** | 0.41 | yes |
| readability | 89% | 4,338 | 0.29 | 0.00 | yes |
| markitdown | 67%* | 4,609 | 0.92 | **0.73** | yes |

\* markitdown's low coverage is 403/404s on Wikipedia and react.dev (no
browser-like headers, no JS render), not a fidelity gap on pages it fetched.

- distill and trafilatura are the only tools with 100% coverage *and* fully
  local, no-API-key operation.
- distill is the fastest end-to-end (648ms avg) and leanest well-formed
  output among tools that reliably fetch every page; trafilatura is close on
  speed and comparable on tokens.
- **distill's weakest point vs the field: table and code-block fidelity.**
  trafilatura keeps 95% of tables vs distill's 36%; markitdown keeps 73% of
  code blocks vs distill's 41%. Both beat distill precisely on the
  "docs/tables" use case distill's own README says agents need most —
  confirms structural fidelity is the right thing to prioritize next, not a
  self-serving claim.
- jina keeps the most tables (0.77) via JS rendering but drops all code
  blocks (0.00) and nearly triples output tokens — a cost/completeness
  tradeoff, not a clean win.
- readability (the classic reader-mode pipeline) is the weakest overall:
  worst coverage (89%), worst link/heading retention, and real
  under-extraction (e.g. py-json-docs: 1,101 tokens vs distill's 7,906) rather
  than genuine efficiency.
- Full per-page numbers: `results/scorecard.md` and `results/results.json`.
  Content-quality (gold-F1) and agent-QA layers still need gold files /
  `ANTHROPIC_API_KEY` to run — open question until then.
