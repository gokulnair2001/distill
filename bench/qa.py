"""Agent-QA layer — the north-star 'agent-ready' metric.

Idea: generate factual Q&A from a trusted reference of the page, then ask an LLM
to answer each question using ONLY a given tool's Markdown. Score how many are
answerable+correct, and the token cost. A tool whose output drops the answer (or
buries it in boilerplate) scores lower / costs more.

Requires ANTHROPIC_API_KEY. Uses the Messages API over plain HTTP (no SDK dep).
"""
from __future__ import annotations

import json
import os

import requests

API_URL = "https://api.anthropic.com/v1/messages"
MODEL = os.environ.get("DISTILL_BENCH_MODEL", "claude-sonnet-5")


def available() -> tuple[bool, str]:
    if not os.environ.get("ANTHROPIC_API_KEY"):
        return False, "ANTHROPIC_API_KEY not set"
    return True, ""


def _call(system: str, user: str, max_tokens: int = 1024) -> str:
    headers = {
        "x-api-key": os.environ["ANTHROPIC_API_KEY"],
        "anthropic-version": "2023-06-01",
        "content-type": "application/json",
    }
    body = {
        "model": MODEL,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{"role": "user", "content": user}],
    }
    r = requests.post(API_URL, headers=headers, json=body, timeout=120)
    r.raise_for_status()
    data = r.json()
    return "".join(b.get("text", "") for b in data.get("content", []))


def generate_qa(reference: str, n: int = 5) -> list[dict]:
    """Return [{question, answer}] grounded in the reference text."""
    system = (
        "You write factual QA pairs to test whether a document contains specific "
        "information. Return ONLY JSON: a list of objects with keys 'question' and "
        "'answer'. Answers must be short and verbatim-grounded in the text."
    )
    user = (
        f"From the following page content, write {n} specific factual questions whose "
        f"answers appear in the text (numbers, names, definitions, steps). "
        f"Content:\n\n{reference[:12000]}"
    )
    out = _call(system, user, max_tokens=1500)
    try:
        start = out.index("[")
        end = out.rindex("]") + 1
        return json.loads(out[start:end])[:n]
    except Exception:  # noqa: BLE001
        return []


def grade(tool_markdown: str, qa: list[dict]) -> dict:
    """Ask the model to answer each question using only the tool output; grade it."""
    correct = 0
    total = len(qa)
    for item in qa:
        q, gold = item.get("question", ""), item.get("answer", "")
        system = (
            "Answer the question using ONLY the provided document. If the answer is "
            "not present, reply exactly 'NOT_FOUND'. Be terse."
        )
        user = f"Document:\n{tool_markdown[:16000]}\n\nQuestion: {q}"
        try:
            ans = _call(system, user, max_tokens=256).strip()
        except Exception:  # noqa: BLE001
            ans = "NOT_FOUND"
        if ans == "NOT_FOUND":
            continue
        # Grade equivalence against the gold answer.
        gsys = "Reply only 'YES' or 'NO': is the candidate answer correct given the reference?"
        guser = f"Question: {q}\nReference answer: {gold}\nCandidate answer: {ans}"
        try:
            verdict = _call(gsys, guser, max_tokens=8).strip().upper()
        except Exception:  # noqa: BLE001
            verdict = "NO"
        if verdict.startswith("YES"):
            correct += 1
    return {
        "questions": total,
        "correct": correct,
        "accuracy": round(correct / total, 3) if total else None,
    }
