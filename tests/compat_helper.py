"""Ayudante para las pruebas de compatibilidad con la GUI de Rust.

Lo ejecuta `gui/core/tests/compat.rs` (y lo importa `test_compat.py`). Usa el código
real de la TUI sobre el HOME/XDG que reciba por entorno. Subcomandos:

  ingest                       simula un hook + la statusline de una sesión
  collect                      imprime los agentes que ve el adaptador (JSON)
  register DIR NAME [N]        registra proyectos (N veces: DIR-i / NAME-i)
  projects                     imprime los proyectos registrados
  ledger SID AID [N]           N altas concurrentes en el libro de subagentes
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from zeroagents.adapters import claude_code as cc  # noqa: E402
from zeroagents.work import live  # noqa: E402
from zeroagents import messages  # noqa: E402
from zeroagents.paths import platform_dir  # noqa: E402

SID = "sesion-compat-1"


def ingest() -> None:
    now = time.time()
    for name, extra in [("SessionStart", {"model": "claude-opus-5-5", "cwd": "/tmp/proyecto-compat"}),
                        ("UserPromptSubmit", {}),
                        ("PreToolUse", {"tool_name": "Bash"}),
                        ("Stop", {"last_assistant_message": "Hecho."})]:
        cc.ingest_hook({"session_id": SID, "hook_event_name": name, **extra}, notify=False)
    cc.ingest_statusline({
        "session_id": SID, "model": {"id": "claude-opus-5-5", "display_name": "Opus"},
        "effort": {"level": "high"}, "cost": {"total_cost_usd": 1.25, "total_duration_ms": 60000},
        "context_window": {"used_percentage": 12, "context_window_size": 200000},
        "workspace": {"current_dir": "/tmp/proyecto-compat"},
        "rate_limits": {"five_hour": {"used_percentage": 30, "resets_at": now + 3600},
                        "seven_day": {"used_percentage": 5, "resets_at": now + 86400}},
    })


def collect() -> list[dict]:
    agents, _ = cc.ClaudeCodeAdapter().collect(include_ended=True)
    return [{"id": a.id, "status": a.status.value, "model": a.model, "effort": a.effort,
             "cost": a.usage.cost_usd, "context_pct": a.usage.context_pct, "cwd": a.cwd,
             "last_message": a.last_message} for a in agents]


def main(argv: list[str]) -> None:
    cmd = argv[0]
    if cmd == "ingest":
        ingest()
    elif cmd == "collect":
        print(json.dumps(collect()))
    elif cmd == "register":
        base, name = argv[1], argv[2]
        count = int(argv[3]) if len(argv) > 3 else 0
        if count:
            for i in range(count):
                live.register(Path(f"{base}-{i}"), f"{name}-{i}")
        else:
            live.register(Path(base), name)
    elif cmd == "projects":
        print(json.dumps([str(p) for p in live.projects()]))
    elif cmd == "ledger":
        led = messages.Ledger(platform_dir("claude-code") / f"{argv[1]}.agents.json")
        for i in range(int(argv[3]) if len(argv) > 3 else 1):
            led.add_background(f"{argv[2]}-{i}")


if __name__ == "__main__":
    main(sys.argv[1:])
