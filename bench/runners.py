"""Tool runners: each takes a URL and returns Markdown + timing.

Every runner returns a RunResult so the harness can score them uniformly.
Cloud runners degrade gracefully when their API key is missing.
"""
from __future__ import annotations

import os
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path

import requests

BENCH_DIR = Path(__file__).resolve().parent
REPO_DIR = BENCH_DIR.parent


@dataclass
class RunResult:
    tool: str
    url: str
    markdown: str = ""
    ms: float = 0.0
    ok: bool = False
    error: str = ""
    meta: dict = field(default_factory=dict)


class Runner:
    name = "base"

    def available(self) -> tuple[bool, str]:
        """(is_available, reason_if_not)."""
        return True, ""

    def run(self, url: str) -> RunResult:  # pragma: no cover - interface
        raise NotImplementedError


class DistillRunner(Runner):
    name = "distill"

    def __init__(self, binary: str | None = None, frontmatter: bool = False):
        self.binary = binary or str(REPO_DIR / "target" / "release" / "distill")
        self.frontmatter = frontmatter

    def available(self) -> tuple[bool, str]:
        if not Path(self.binary).exists():
            return False, f"binary not found at {self.binary} (run `cargo build --release`)"
        return True, ""

    def run_stdin(self, html: str) -> str:
        """Convert raw HTML via stdin — used for isolated determinism checks."""
        args = [self.binary, "-", "--no-frontmatter"]
        try:
            proc = subprocess.run(args, input=html, capture_output=True,
                                  text=True, timeout=90)
            return proc.stdout
        except Exception:  # noqa: BLE001
            return ""

    def run(self, url: str) -> RunResult:
        args = [self.binary, url]
        if not self.frontmatter:
            args.append("--no-frontmatter")
        t = time.perf_counter()
        try:
            proc = subprocess.run(
                args, capture_output=True, text=True, timeout=90
            )
            ms = (time.perf_counter() - t) * 1000
            if proc.returncode != 0:
                return RunResult(self.name, url, ms=ms, ok=False,
                                 error=proc.stderr.strip()[:300])
            md = proc.stdout
            return RunResult(self.name, url, markdown=md, ms=ms, ok=bool(md.strip()))
        except subprocess.TimeoutExpired:
            return RunResult(self.name, url, ms=90000, ok=False, error="timeout")
        except Exception as e:  # noqa: BLE001
            return RunResult(self.name, url, ok=False, error=str(e)[:300])


class JinaRunner(Runner):
    name = "jina"

    def __init__(self):
        self.key = os.environ.get("JINA_API_KEY", "")

    def available(self) -> tuple[bool, str]:
        return True, ""  # free endpoint works without a key (rate-limited)

    def run(self, url: str) -> RunResult:
        headers = {"X-Return-Format": "markdown"}
        if self.key:
            headers["Authorization"] = f"Bearer {self.key}"
        endpoint = f"https://r.jina.ai/{url}"
        t = time.perf_counter()
        try:
            resp = requests.get(endpoint, headers=headers, timeout=90)
            ms = (time.perf_counter() - t) * 1000
            if resp.status_code != 200:
                return RunResult(self.name, url, ms=ms, ok=False,
                                 error=f"HTTP {resp.status_code}: {resp.text[:150]}")
            md = resp.text
            return RunResult(self.name, url, markdown=md, ms=ms, ok=bool(md.strip()))
        except Exception as e:  # noqa: BLE001
            return RunResult(self.name, url, ok=False, error=str(e)[:300])


class FirecrawlRunner(Runner):
    name = "firecrawl"

    def __init__(self):
        self.key = os.environ.get("FIRECRAWL_API_KEY", "")

    def available(self) -> tuple[bool, str]:
        if not self.key:
            return False, "FIRECRAWL_API_KEY not set"
        return True, ""

    def run(self, url: str) -> RunResult:
        headers = {
            "Authorization": f"Bearer {self.key}",
            "Content-Type": "application/json",
        }
        payload = {"url": url, "formats": ["markdown"]}
        t = time.perf_counter()
        try:
            resp = requests.post(
                "https://api.firecrawl.dev/v1/scrape",
                headers=headers, json=payload, timeout=120,
            )
            ms = (time.perf_counter() - t) * 1000
            if resp.status_code != 200:
                return RunResult(self.name, url, ms=ms, ok=False,
                                 error=f"HTTP {resp.status_code}: {resp.text[:150]}")
            data = resp.json()
            md = (data.get("data") or {}).get("markdown", "")
            return RunResult(self.name, url, markdown=md, ms=ms, ok=bool(md.strip()))
        except Exception as e:  # noqa: BLE001
            return RunResult(self.name, url, ok=False, error=str(e)[:300])


ALL_RUNNERS = {
    "distill": DistillRunner,
    "jina": JinaRunner,
    "firecrawl": FirecrawlRunner,
}
