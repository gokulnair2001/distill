# distill benchmark harness

Scores `distill` against cloud competitors (**Jina Reader**, **Firecrawl**) over a
bucketed URL corpus, across five layers. Its job is to turn "the output looks good"
into defensible numbers — and to expose where distill loses.

## Quick start

```bash
cd bench
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
./.venv/bin/python bench.py --tools distill,jina        # runs now, no keys
```

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

## Known findings (initial distill vs Jina run, 9 URLs)

- distill: ~2× faster, ~½ the tokens, deterministic.
- Jina: wins SPA coverage (renders JS), keeps more tables/images (+ more boilerplate).
- Open question: true content quality — resolved only once gold-F1 / QA layers run.
