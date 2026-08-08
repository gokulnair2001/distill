"""Scoring metrics for extracted Markdown.

Layers:
  - basics:      coverage, speed, determinism
  - structural:  link / table / code / heading preservation vs source HTML
  - efficiency:  token cost + (with gold) boilerplate ratio
  - content:     token-level P/R/F1 and ROUGE-L vs gold
"""
from __future__ import annotations

import re
from functools import lru_cache

import requests
from bs4 import BeautifulSoup

UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")

# ---------------------------------------------------------------- tokenization

def est_tokens(text: str) -> int:
    """Approximate LLM token count. Uses tiktoken if available, else chars/4.

    Relative comparison across tools is what matters here; the proxy is fine.
    """
    try:  # optional, better if present
        import tiktoken  # type: ignore
        enc = tiktoken.get_encoding("cl100k_base")
        return len(enc.encode(text))
    except Exception:  # noqa: BLE001
        return max(1, round(len(text) / 4))


def words(text: str) -> list[str]:
    return re.findall(r"[a-z0-9]+", text.lower())


# ------------------------------------------------------------- markdown parsing

_LINK = re.compile(r"(?<!\!)\[[^\]]+\]\([^)]+\)")
_IMG = re.compile(r"!\[[^\]]*\]\([^)]+\)")
_FENCE = re.compile(r"^```", re.MULTILINE)
_HEADING = re.compile(r"^#{1,6}\s", re.MULTILINE)
_TABLE_SEP = re.compile(r"^\s*\|?\s*:?-{2,}", re.MULTILINE)


def md_features(md: str) -> dict:
    return {
        "links": len(_LINK.findall(md)),
        "images": len(_IMG.findall(md)),
        "code_blocks": len(_FENCE.findall(md)) // 2,
        "headings": len(_HEADING.findall(md)),
        "tables": len(_TABLE_SEP.findall(md)),
    }


# ------------------------------------------------------- source HTML "expected"

@lru_cache(maxsize=256)
def fetch_html(url: str) -> str:
    try:
        r = requests.get(url, headers={"User-Agent": UA}, timeout=60)
        return r.text if r.status_code == 200 else ""
    except Exception:  # noqa: BLE001
        return ""


def expected_features(html: str) -> dict:
    """Upper-bound feature counts from the source, minus obvious chrome.

    Not perfect ground truth, but identical for every tool on the same page,
    so preservation *ratios* are comparable across tools.
    """
    if not html:
        return {"links": 0, "images": 0, "code_blocks": 0, "headings": 0, "tables": 0}
    soup = BeautifulSoup(html, "html.parser")
    for tag in soup(["script", "style", "noscript", "nav", "footer", "aside", "header"]):
        tag.decompose()
    body = soup.body or soup
    return {
        "links": len(body.find_all("a", href=True)),
        "images": len(body.find_all("img")),
        "code_blocks": len(body.find_all("pre")),
        "headings": len(body.find_all(re.compile("^h[1-6]$"))),
        "tables": len(body.find_all("table")),
    }


def structural_scores(md: str, expected: dict) -> dict:
    """Per-feature preservation ratio min(out/expected, 1). 1.0 = kept all."""
    got = md_features(md)
    out = {}
    ratios = []
    for k, exp in expected.items():
        if exp <= 0:
            out[k] = None  # feature absent in source; not scored
            continue
        r = min(got[k] / exp, 1.0)
        out[k] = round(r, 3)
        ratios.append(r)
    out["_mean"] = round(sum(ratios) / len(ratios), 3) if ratios else None
    out["_raw"] = got
    return out


# --------------------------------------------------------------- content vs gold

def prf1(pred: str, gold: str) -> dict:
    """Token-level precision / recall / F1 (multiset over word tokens)."""
    from collections import Counter
    p = Counter(words(pred))
    g = Counter(words(gold))
    if not p or not g:
        return {"precision": 0.0, "recall": 0.0, "f1": 0.0}
    overlap = sum((p & g).values())
    precision = overlap / max(sum(p.values()), 1)
    recall = overlap / max(sum(g.values()), 1)
    f1 = 2 * precision * recall / (precision + recall) if (precision + recall) else 0.0
    return {"precision": round(precision, 3), "recall": round(recall, 3), "f1": round(f1, 3)}


def rouge_l(pred: str, gold: str) -> float:
    """ROUGE-L F-measure via LCS over word sequences."""
    a, b = words(pred), words(gold)
    if not a or not b:
        return 0.0
    # LCS length (space-optimized DP)
    prev = [0] * (len(b) + 1)
    for x in a:
        cur = [0]
        for j, y in enumerate(b, 1):
            cur.append(prev[j - 1] + 1 if x == y else max(prev[j], cur[-1]))
        prev = cur
    lcs = prev[-1]
    prec = lcs / len(a)
    rec = lcs / len(b)
    return round(2 * prec * rec / (prec + rec), 3) if (prec + rec) else 0.0


def content_scores(md: str, gold: str) -> dict:
    d = prf1(md, gold)
    d["rouge_l"] = rouge_l(md, gold)
    # Boilerplate ratio: output tokens per gold-content token (>1 = bloat).
    gt = est_tokens(gold)
    d["bloat_ratio"] = round(est_tokens(md) / gt, 2) if gt else None
    return d


def coverage_ok(md: str, min_chars: int = 200) -> bool:
    return len(md.strip()) >= min_chars
