#!/usr/bin/env python3
"""distill benchmark harness.

Runs distill and cloud competitors (Jina, Firecrawl) over a bucketed URL corpus
and scores them across layers: basics, structural, efficiency, content, qa.

Usage:
    python bench.py                         # all available tools, all doable layers
    python bench.py --tools distill,jina    # pick tools
    python bench.py --layers basics,structural
    python bench.py --limit 3               # first 3 URLs
    python bench.py --qa-n 5                 # QA questions per page (needs key)
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path

import metrics
import qa as qa_mod
from runners import ALL_RUNNERS

BENCH_DIR = Path(__file__).resolve().parent
GOLD_DIR = BENCH_DIR / "corpus" / "gold"
RESULTS_DIR = BENCH_DIR / "results"
RAW_DIR = RESULTS_DIR / "raw"

ALL_LAYERS = ["basics", "structural", "efficiency", "content", "qa"]


def load_corpus(path: Path, limit: int | None) -> list[dict]:
    rows = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    return rows[:limit] if limit else rows


def gold_for(cid: str) -> str | None:
    f = GOLD_DIR / f"{cid}.txt"
    return f.read_text() if f.exists() else None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tools", default="distill,jina,firecrawl")
    ap.add_argument("--layers", default="all")
    ap.add_argument("--corpus", default=str(BENCH_DIR / "corpus.jsonl"))
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--qa-n", type=int, default=5)
    ap.add_argument("--trials", type=int, default=5,
                    help="trials for distill's process-only timing (median)")
    args = ap.parse_args()

    layers = ALL_LAYERS if args.layers == "all" else args.layers.split(",")
    tool_names = args.tools.split(",")
    corpus = load_corpus(Path(args.corpus), args.limit)
    RAW_DIR.mkdir(parents=True, exist_ok=True)

    # Instantiate available runners.
    runners = []
    for name in tool_names:
        if name not in ALL_RUNNERS:
            print(f"! unknown tool: {name}")
            continue
        r = ALL_RUNNERS[name]()
        ok, why = r.available()
        if not ok:
            print(f"- skipping {name}: {why}")
            continue
        runners.append(r)
    if not runners:
        print("No runners available. Aborting.")
        return

    qa_ok, qa_why = qa_mod.available()
    if "qa" in layers and not qa_ok:
        print(f"- qa layer disabled: {qa_why}")

    per_tool: dict[str, list[dict]] = {r.name: [] for r in runners}

    distill_runner = next((r for r in runners if r.name == "distill"), None)

    for row in corpus:
        cid, bucket, url = row["id"], row["bucket"], row["url"]
        print(f"\n=== [{bucket}] {cid} — {url}")
        # Fetch source HTML once (cached) if needed for structural scoring or
        # distill's process-only timing.
        need_html = ("structural" in layers) or (distill_runner is not None)
        html = metrics.fetch_html(url) if need_html else ""
        expected = metrics.expected_features(html) if ("structural" in layers and html) else None
        gold = gold_for(cid)

        outputs: dict[str, str] = {}
        for r in runners:
            res = r.run(url)
            (RAW_DIR / f"{cid}.{r.name}.md").write_text(res.markdown or "")
            row_metrics = {"id": cid, "bucket": bucket, "ok": res.ok,
                           "ms": round(res.ms, 1), "error": res.error}

            if res.ok:
                outputs[r.name] = res.markdown
                if "basics" in layers:
                    row_metrics["coverage"] = metrics.coverage_ok(res.markdown)
                if "structural" in layers and expected:
                    row_metrics["structural"] = metrics.structural_scores(res.markdown, expected)
                if "efficiency" in layers:
                    row_metrics["tokens"] = metrics.est_tokens(res.markdown)
                    # Quality-gated "page read" for pages-per-token scoring.
                    # Prefer structural when present so empty code-dropping
                    # dumps don't count as useful reads.
                    row_metrics["gated"] = metrics.gated_ok(
                        res.markdown,
                        row_metrics.get("structural"),
                        expected,
                    )
                if "content" in layers and gold:
                    row_metrics["content"] = metrics.content_scores(res.markdown, gold)
            else:
                if "basics" in layers:
                    row_metrics["coverage"] = False
                if "efficiency" in layers:
                    row_metrics["gated"] = False
            per_tool[r.name].append(row_metrics)
            status = "ok" if res.ok else f"FAIL ({res.error[:60]})"
            extra = f" {row_metrics.get('tokens','')}tok" if res.ok else ""
            print(f"   {r.name:10} {res.ms:7.0f}ms  {status}{extra}")

        # Process-only timing for distill: run on the SAME cached HTML (no
        # network), median of N trials. This is the stable, defensible speed
        # number — the end-to-end `ms` above is dominated by network variance.
        if distill_runner and html and hasattr(distill_runner, "run_stdin"):
            import time as _time
            trials = []
            first = None
            for _ in range(max(1, args.trials)):
                t = _time.perf_counter()
                md_out = distill_runner.run_stdin(html)
                trials.append((_time.perf_counter() - t) * 1000)
                if first is None:
                    first = md_out
            per_tool["distill"][-1]["proc_ms"] = round(statistics.median(trials), 1)
            # Determinism: identical output across trials on identical input.
            second = distill_runner.run_stdin(html)
            per_tool["distill"][-1]["deterministic"] = bool(first) and (first == second)
            print(f"   distill    proc-only median: {per_tool['distill'][-1]['proc_ms']}ms")

        # QA layer (needs key + a reference).
        if "qa" in layers and qa_ok and outputs:
            reference = gold or max(outputs.values(), key=len)
            qapairs = qa_mod.generate_qa(reference, n=args.qa_n)
            if qapairs:
                for name, md in outputs.items():
                    g = qa_mod.grade(md, qapairs)
                    idx = next(i for i, m in enumerate(per_tool[name])
                               if m["id"] == cid)
                    per_tool[name][idx]["qa"] = g
                    print(f"   {name:10} QA {g['correct']}/{g['questions']}")

    report(per_tool, layers)
    RESULTS_DIR.mkdir(exist_ok=True)
    (RESULTS_DIR / "results.json").write_text(json.dumps(per_tool, indent=2))
    print(f"\nRaw outputs: {RAW_DIR}\nResults JSON: {RESULTS_DIR / 'results.json'}")


def _avg(vals: list[float]) -> float | None:
    vals = [v for v in vals if v is not None]
    return round(statistics.mean(vals), 3) if vals else None


def _common_gated_ids(per_tool: dict[str, list[dict]]) -> set[str]:
    """Page ids that every tool quality-gated (fair same-page efficiency set)."""
    ids_sets: list[set[str]] = []
    for rows in per_tool.values():
        if not rows:
            continue
        ids_sets.append({r["id"] for r in rows if r.get("gated")})
    if not ids_sets:
        return set()
    return set.intersection(*ids_sets)

def report(per_tool: dict[str, list[dict]], layers: list[str]) -> None:
    print("\n" + "=" * 70)
    print("SCORECARD (averaged over corpus)")
    print("=" * 70)

    cols = ["tool", "n", "cover%", "e2e_ms", "proc_ms"]
    if "efficiency" in layers:
        cols += ["avg_tok", "gated%", "tok/page", "pages/1k"]
    if "structural" in layers:
        cols.append("struct")
    if "content" in layers:
        cols += ["F1", "rougeL", "bloat"]
    if "qa" in layers:
        cols.append("qa_acc")
    cols.append("determ")

    print("| " + " | ".join(cols) + " |")
    print("|" + "|".join("---" for _ in cols) + "|")

    for tool, rows in per_tool.items():
        if not rows:
            continue
        n = len(rows)
        cover = sum(1 for r in rows if r.get("coverage")) / n * 100
        proc = _avg([r.get("proc_ms") for r in rows])
        cells = [
            tool,
            str(n),
            f"{cover:.0f}",
            f"{_avg([r['ms'] for r in rows]):.0f}",
            f"{proc:.0f}" if proc is not None else "-",
        ]
        if "efficiency" in layers:
            eff = metrics.efficiency_pages_per_token(rows)
            cells.append(str(_avg([r.get("tokens") for r in rows]) or "-"))
            cells.append(f"{eff['gated_pct']:.0f}")
            cells.append(
                f"{eff['tok_per_page']:.0f}" if eff["tok_per_page"] is not None else "-"
            )
            cells.append(
                f"{eff['pages_per_1k']:.3f}" if eff["pages_per_1k"] is not None else "-"
            )
        if "structural" in layers:
            cells.append(str(_avg([r.get("structural", {}).get("_mean") for r in rows]) or "-"))
        if "content" in layers:
            cells.append(str(_avg([r.get("content", {}).get("f1") for r in rows]) or "-"))
            cells.append(str(_avg([r.get("content", {}).get("rouge_l") for r in rows]) or "-"))
            cells.append(str(_avg([r.get("content", {}).get("bloat_ratio") for r in rows]) or "-"))
        if "qa" in layers:
            cells.append(str(_avg([r.get("qa", {}).get("accuracy") for r in rows]) or "-"))
        det = [r.get("deterministic") for r in rows if "deterministic" in r]
        cells.append("yes" if det and all(det) else ("no" if det else "-"))
        print("| " + " | ".join(cells) + " |")

    if "efficiency" in layers:
        print("\nQuality-gated efficiency — pages read per 1k output tokens")
        print("(higher pages/1k is better; gate = ≥200 chars, and if source had")
        print("code blocks keep ≥1. Stubs/failures count as 0 pages. tok/page is")
        print("mean tokens on gated pages only. Common-set = pages every tool gated.)")
        print("| tool | gated | gated% | tok/page | pages/1k | pages/1k (common) |")
        print("|---|---|---|---|---|---|")
        common = _common_gated_ids(per_tool)
        for tool, rows in per_tool.items():
            if not rows:
                continue
            eff = metrics.efficiency_pages_per_token(rows)
            common_rows = [r for r in rows if r.get("id") in common]
            # On the common set every tool gated; score tokens only.
            common_eff = metrics.efficiency_pages_per_token(
                [{**r, "gated": True} for r in common_rows if r.get("tokens") is not None]
            ) if common_rows else {"pages_per_1k": None}
            cells = [
                tool,
                str(eff["gated_pages"]),
                f"{eff['gated_pct']:.0f}",
                f"{eff['tok_per_page']:.0f}" if eff["tok_per_page"] is not None else "-",
                f"{eff['pages_per_1k']:.3f}" if eff["pages_per_1k"] is not None else "-",
                f"{common_eff['pages_per_1k']:.3f}" if common_eff.get("pages_per_1k") is not None else "-",
            ]
            print("| " + " | ".join(cells) + " |")

    if "structural" in layers:
        print("\nStructural preservation by feature — MICRO-average Σkept/Σsource")
        print("(cells show ratio [Σkept/Σsource over N pages]; links/headings "
              "include boilerplate so higher isn't always better):")
        feats = ["tables", "code_blocks", "links", "headings", "images"]
        print("| tool | " + " | ".join(feats) + " |")
        print("|" + "|".join("---" for _ in range(len(feats) + 1)) + "|")
        for tool, rows in per_tool.items():
            if not rows:
                continue
            cells = [tool]
            for f in feats:
                got = exp = pages = 0
                for r in rows:
                    s = r.get("structural") or {}
                    e = (s.get("_expected") or {}).get(f, 0)
                    g = (s.get("_raw") or {}).get(f, 0)
                    if e > 0:
                        got += g
                        exp += e
                        pages += 1
                if exp > 0:
                    cells.append(f"{got/exp:.2f} [{got}/{exp} n={pages}]")
                else:
                    cells.append("-")
            print("| " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
