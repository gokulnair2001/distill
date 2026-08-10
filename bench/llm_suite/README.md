# Distill LLM fact-check suite

12-URL corpus that generates **Distill-only** outputs for LLM checks: plain
Markdown + `--agent-ready` JSON.

## Setup

```bash
cargo build --release
# Python 3 is enough — no bench venv required
```

## Generate Distill outputs

```bash
cd bench
python3 llm_suite/generate.py
```

Writes to **`llm_suite/out/`** (gitignored):

```
out/
  urls.json
  <id>/
    distill.md
    distill.agent-ready.json
```

`urls.json` maps each page id → URL / bucket / ok·ms·chars for the two Distill
outputs. Do not commit `out/` — regenerate locally when you need fresh fixtures.
