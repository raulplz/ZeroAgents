"""Abrir el líder del equipo en una terminal nueva.

Los equipos de Claude Code son experimentales: se activan con
`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`, que aquí se pone solo para el proceso
del líder, sin tocar `~/.claude/settings.json`.
"""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
from pathlib import Path

from zeroagents.work import plan as P
from zeroagents.work.scaffold import LEAD_PROMPT_FILE

KICKOFF = f"Lee {LEAD_PROMPT_FILE.as_posix()} y sigue sus instrucciones."


def lead_command(data: dict, permission_mode: str | None = None) -> list[str]:
    lead = data["plan"]["lead"]
    cmd = ["claude", "--model", lead["model"], "--effort", lead["effort"]]
    if permission_mode:
        cmd += ["--permission-mode", permission_mode]
    return cmd + [KICKOFF]


def launch(directory: Path, permission_mode: str | None = None) -> str:
    """Abre la terminal con el líder. Devuelve la orden que ha usado."""
    data = P.load(directory)
    if not data:
        raise RuntimeError(f"No hay plan en {directory / P.PLAN_FILE}. Crea uno con `zeroagents work new`.")
    if permission_mode is None:   # lo elegido al crear el proyecto (formulario del panel)
        permission_mode = (data.get("launch") or {}).get("permission_mode")
        if permission_mode == "default":
            permission_mode = None
    if not (directory / LEAD_PROMPT_FILE).exists():
        raise RuntimeError(f"Falta {LEAD_PROMPT_FILE}.")
    inner = ["env", "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1", *lead_command(data, permission_mode)]
    title = f"ZeroAgents · {data['plan']['project']['name']}"
    if shutil.which("xdg-terminal-exec"):
        cmd = ["xdg-terminal-exec", f"--dir={directory}", f"--title={title}", "--", *inner]
        if shutil.which("uwsm-app"):     # como lanza las apps Omarchy
            cmd = ["uwsm-app", "--", *cmd]
    elif term := os.environ.get("TERMINAL"):
        cmd = [term, "-e", *inner]
    else:
        raise RuntimeError("No encuentro una terminal (xdg-terminal-exec o $TERMINAL). Ejecuta a mano:\n"
                           f"cd {shlex.quote(str(directory))} && {shlex.join(inner)}")
    subprocess.Popen(cmd, cwd=directory, start_new_session=True,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return shlex.join(inner)
