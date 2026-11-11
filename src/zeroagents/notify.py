"""Notificaciones de escritorio. Si no hay forma de notificar, no hace nada."""

from __future__ import annotations

import shutil
import subprocess
import sys


def send(title: str, body: str, urgent: bool = False) -> None:
    try:
        if sys.platform.startswith("linux") and shutil.which("notify-send"):
            cmd = ["notify-send", "-a", "ZeroAgents", "-i", "dialog-information"]
            if urgent:
                cmd += ["-u", "critical"]
            subprocess.run([*cmd, title, body], timeout=5, check=False)
        elif sys.platform == "darwin":
            script = f"display notification {_q(body)} with title {_q(title)}"
            subprocess.run(["osascript", "-e", script], timeout=5, check=False)
    except (OSError, subprocess.SubprocessError):
        pass


def _q(text: str) -> str:
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'
