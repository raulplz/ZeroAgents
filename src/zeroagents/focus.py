"""Saltar a la ventana de terminal de un agente (de momento, solo Hyprland)."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path


def _ppid(pid: int) -> int | None:
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    return int(stat.rsplit(")", 1)[1].split()[1])


def focus_pid(pid: int | None) -> str | None:
    """Enfoca la ventana que contiene el proceso. Devuelve un error o None."""
    if not pid:
        return "Este agente no tiene proceso asociado."
    if not (shutil.which("hyprctl") and os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")):
        return "Saltar a la ventana solo funciona en Hyprland por ahora."
    try:
        out = subprocess.run(["hyprctl", "clients", "-j"], capture_output=True, text=True, timeout=3)
        windows = {c["pid"]: c["address"] for c in json.loads(out.stdout)}
    except (OSError, ValueError, subprocess.SubprocessError, KeyError):
        return "No he podido consultar las ventanas de Hyprland."
    current: int | None = pid
    while current and current > 1:
        if current in windows:
            return _focus_address(windows[current])
        current = _ppid(current)
    return "No encuentro la ventana de este agente (¿está en tmux o en otra sesión?)."


def _focus_address(address: str) -> str | None:
    # Hyprland con configuración Lua usa hl.dsp.*; las versiones anteriores, la sintaxis clásica.
    attempts = (
        ["hyprctl", "dispatch", f'hl.dsp.focus({{ window = "address:{address}" }})'],
        ["hyprctl", "dispatch", "focuswindow", f"address:{address}"],
    )
    for cmd in attempts:
        try:
            out = subprocess.run(cmd, capture_output=True, text=True, timeout=3)
        except (OSError, subprocess.SubprocessError):
            continue
        if out.returncode == 0 and out.stdout.strip() == "ok":
            return None
    return "Hyprland no ha aceptado la orden de enfocar la ventana."
