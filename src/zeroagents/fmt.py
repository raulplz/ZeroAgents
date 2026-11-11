"""Formato compartido por la CLI y el panel."""

from __future__ import annotations

import time
from pathlib import Path

from zeroagents.adapters.claude_code import pretty_model
from zeroagents.model import Status

MODES = {
    "default": "normal",
    "manual": "normal",
    "acceptEdits": "editar",
    "plan": "plan",
    "auto": "auto",
    "dontAsk": "no preguntar",
    "bypassPermissions": "sin permisos",
}
EFFORTS = {"low": "bajo", "medium": "medio", "high": "alto", "xhigh": "muy alto", "max": "máximo"}
PLATFORMS = {"claude-code": "Claude"}
ICONS = {
    Status.PERMISSION: "●",
    Status.QUESTION: "●",
    Status.WORKING: "◐",
    Status.YOUR_TURN: "○",
    Status.UNKNOWN: "?",
    Status.ENDED: "·",
}


def mode(value: str | None) -> str:
    return MODES.get(value or "", value or "—")


def effort(value: str | None) -> str:
    return EFFORTS.get(value or "", value or "—")


def model(value: str | None) -> str:
    return pretty_model(value) or "—"


def tokens(n: int | None) -> str:
    if not n:
        return "—"
    for size, suffix in ((1e9, "G"), (1e6, "M"), (1e3, "k")):
        if n >= size:
            return f"{n / size:.1f}{suffix}".replace(".0", "")
    return str(n)


def cost(value: float | None) -> str:
    return "—" if value is None else f"${value:.2f}"


def ago(epoch: float | None, now: float | None = None) -> str:
    if not epoch:
        return "—"
    s = max(0, int((now or time.time()) - epoch))
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{s // 60}m"
    if s < 86400:
        return f"{s // 3600}h {s % 3600 // 60:02d}m"
    return f"{s // 86400}d"


def duration(ms: int | None) -> str:
    if not ms:
        return "—"
    return ago(time.time() - ms / 1000)


def place(cwd: str) -> str:
    if not cwd:
        return "—"
    home = str(Path.home())
    return "~" + cwd[len(home):] if cwd.startswith(home) else cwd


def bar(pct: float | None, cells: int = 5) -> str:
    if pct is None:
        return "—"
    # ━ y ─ ocupan una sola celda en cualquier fuente (▰▱ no).
    filled = round(max(0.0, min(100.0, pct)) / 100 * cells)
    return "━" * filled + "─" * (cells - filled) + f" {pct:.0f}%"


def clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: width - 1] + "…"


def clock(epoch: float | None) -> str:
    return time.strftime("%H:%M", time.localtime(epoch)) if epoch else "—"
