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


class TrafilaturaRunner(Runner):
    """Local, offline extraction library — no network beyond the initial fetch."""
    name = "trafilatura"

    def available(self) -> tuple[bool, str]:
        try:
            import trafilatura  # noqa: F401
        except ImportError:
            return False, "pip install trafilatura"
        return True, ""

    def run(self, url: str) -> RunResult:
        import trafilatura
        t = time.perf_counter()
        try:
            downloaded = trafilatura.fetch_url(url)
            if not downloaded:
                return RunResult(self.name, url, ms=(time.perf_counter() - t) * 1000,
                                 ok=False, error="fetch_url returned nothing")
            md = trafilatura.extract(
                downloaded, output_format="markdown",
                include_tables=True, include_links=True, include_images=True,
                with_metadata=True,
            ) or ""
            ms = (time.perf_counter() - t) * 1000
            return RunResult(self.name, url, markdown=md, ms=ms, ok=bool(md.strip()))
        except Exception as e:  # noqa: BLE001
            return RunResult(self.name, url, ok=False, error=str(e)[:300])


class ReadabilityRunner(Runner):
    """The classic Mozilla-Readability-style pipeline: readability-lxml for
    content selection, html2text for HTML->Markdown conversion. What most
    "reader mode" tools are built on under the hood."""
    name = "readability"

    def available(self) -> tuple[bool, str]:
        try:
            import readability  # noqa: F401
            import html2text  # noqa: F401
        except ImportError:
            return False, "pip install readability-lxml html2text"
        return True, ""

    def run(self, url: str) -> RunResult:
        import html2text
        import readability
        t = time.perf_counter()
        try:
            resp = requests.get(
                url, timeout=30,
                headers={"User-Agent": "Mozilla/5.0 (compatible; distill-bench/1.0)"},
            )
            resp.raise_for_status()
            doc = readability.Document(resp.text)
            content_html = doc.summary()
            h = html2text.HTML2Text()
            h.body_width = 0
            h.ignore_images = False
            h.ignore_links = False
            md = h.handle(content_html)
            ms = (time.perf_counter() - t) * 1000
            return RunResult(self.name, url, markdown=md, ms=ms, ok=bool(md.strip()))
        except Exception as e:  # noqa: BLE001
            return RunResult(self.name, url, ok=False, error=str(e)[:300])


class MarkitdownRunner(Runner):
    """Microsoft's markitdown — direct competitor: converts pages/docs to
    agent-ready Markdown. No boilerplate stripping (whole-page conversion)."""
    name = "markitdown"

    def available(self) -> tuple[bool, str]:
        try:
            from markitdown import MarkItDown  # noqa: F401
        except ImportError:
            return False, "pip install markitdown"
        return True, ""

    def run(self, url: str) -> RunResult:
        from markitdown import MarkItDown
        t = time.perf_counter()
        try:
            md_converter = MarkItDown()
            result = md_converter.convert(url)
            md = result.text_content or ""
            ms = (time.perf_counter() - t) * 1000
            return RunResult(self.name, url, markdown=md, ms=ms, ok=bool(md.strip()))
        except Exception as e:  # noqa: BLE001
            return RunResult(self.name, url, ok=False, error=str(e)[:300])


ALL_RUNNERS = {
    "distill": DistillRunner,
    "jina": JinaRunner,
    "firecrawl": FirecrawlRunner,
    "trafilatura": TrafilaturaRunner,
    "readability": ReadabilityRunner,
    "markitdown": MarkitdownRunner,
}
