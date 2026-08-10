#!/usr/bin/env python3
"""Generate Distill Markdown + agent-ready JSON for LLM fact checks.

Writes into ``out/`` (gitignored):

    out/
      urls.json
      <id>/
        distill.md
        distill.agent-ready.json

Usage:

    cargo build --release
    cd bench && ./.venv/bin/python llm_suite/generate.py
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

SUITE_DIR = Path(__file__).resolve().parent
BENCH_DIR = SUITE_DIR.parent
REPO_DIR = BENCH_DIR.parent
OUT_DIR = SUITE_DIR / "out"
CORPUS = SUITE_DIR / "corpus.jsonl"


def find_distill_binary() -> str:
    """Prefer DISTILL_BIN, then release/debug under REPO or CARGO_TARGET_DIR."""
    candidates: list[Path] = []
    if env := os.environ.get("DISTILL_BIN"):
        candidates.append(Path(env))
    if ctd := os.environ.get("CARGO_TARGET_DIR"):
        candidates.append(Path(ctd) / "release" / "distill")
        candidates.append(Path(ctd) / "debug" / "distill")
    candidates.append(REPO_DIR / "target" / "release" / "distill")
    candidates.append(REPO_DIR / "target" / "debug" / "distill")
    for path in candidates:
        if path.is_file():
            return str(path)
    return str(REPO_DIR / "target" / "release" / "distill")


def load_corpus(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def run_distill(binary: str, url: str, agent_ready: bool) -> tuple[bool, str, float, str]:
    """Return (ok, body, ms, error)."""
    args = [binary, url, "--no-frontmatter"]
    if agent_ready:
        args.append("--agent-ready")
    t = time.perf_counter()
    try:
        proc = subprocess.run(args, capture_output=True, text=True, timeout=120)
        ms = (time.perf_counter() - t) * 1000
        if proc.returncode != 0:
            return False, "", ms, (proc.stderr or proc.stdout)[:300]
        body = proc.stdout
        return bool(body.strip()), body, ms, ""
    except subprocess.TimeoutExpired:
        return False, "", 120_000.0, "timeout"
    except Exception as e:  # noqa: BLE001
        return False, "", 0.0, str(e)[:300]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--corpus", type=Path, default=CORPUS, help="Path to corpus.jsonl")
    ap.add_argument("--out", type=Path, default=OUT_DIR, help="Output directory (gitignored)")
    ap.add_argument("--limit", type=int, default=0, help="Optional cap (0 = all)")
    ap.add_argument(
        "--binary",
        default=None,
        help="Path to distill binary (default: auto-detect / DISTILL_BIN)",
    )
    args = ap.parse_args()

    corpus = load_corpus(args.corpus)
    if args.limit > 0:
        corpus = corpus[: args.limit]
    if not corpus:
        print("corpus is empty", file=sys.stderr)
        return 2

    binary = args.binary or find_distill_binary()
    if not Path(binary).is_file():
        print(
            f"distill binary not found at {binary} (run `cargo build --release`)",
            file=sys.stderr,
        )
        return 1

    help_txt = subprocess.run(
        [binary, "--help"], capture_output=True, text=True
    ).stdout
    if "--agent-ready" not in help_txt:
        print(
            "warning: distill binary lacks --agent-ready; "
            "rebuild with `cargo build --release` on a branch that has it",
            file=sys.stderr,
        )

    args.out.mkdir(parents=True, exist_ok=True)
    manifest: dict = {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "binary": binary,
        "pages": {},
    }

    print(f"corpus={len(corpus)} binary={binary} out={args.out}")
    for row in corpus:
        page_id = row["id"]
        url = row["url"]
        bucket = row.get("bucket", "")
        page_dir = args.out / page_id
        page_dir.mkdir(parents=True, exist_ok=True)

        entry: dict = {"id": page_id, "bucket": bucket, "url": url, "outputs": {}}
        print(f"\n== {page_id} ({bucket}) ==")
        print(f"   {url}")

        ok, body, ms, err = run_distill(binary, url, agent_ready=False)
        md_status = {
            "ok": ok,
            "ms": round(ms, 1),
            "chars": len(body) if ok else 0,
            "error": err or None,
        }
        if ok:
            write_text(page_dir / "distill.md", body)
            print(f"   distill.md   ok  {md_status['ms']:8.1f} ms  {md_status['chars']} chars")
        else:
            write_text(page_dir / "distill.error.txt", err or "unknown error")
            print(f"   distill.md   FAIL {err}")
        entry["outputs"]["distill.md"] = md_status

        ok, body, ms, err = run_distill(binary, url, agent_ready=True)
        ars_status = {
            "ok": ok,
            "ms": round(ms, 1),
            "chars": len(body) if ok else 0,
            "error": err or None,
        }
        if ok:
            write_text(page_dir / "distill.agent-ready.json", body)
            print(
                f"   agent-ready  ok  {ars_status['ms']:8.1f} ms  {ars_status['chars']} chars"
            )
        else:
            write_text(
                page_dir / "distill.agent-ready.error.txt",
                err or "unknown error",
            )
            print(f"   agent-ready  FAIL {err}")
        entry["outputs"]["distill.agent-ready.json"] = ars_status

        manifest["pages"][page_id] = entry

    urls_path = args.out / "urls.json"
    write_text(urls_path, json.dumps(manifest, indent=2) + "\n")
    print(f"\nwrote {urls_path}")
    print(f"pages under {args.out}/<id>/distill.md + distill.agent-ready.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
