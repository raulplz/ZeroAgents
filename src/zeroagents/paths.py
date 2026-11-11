"""Rutas de estado multiplataforma, sin dependencias externas."""

from __future__ import annotations

import os
import sys
from pathlib import Path


def state_dir() -> Path:
    if override := os.environ.get("ZEROAGENTS_STATE_DIR"):
        return Path(override)
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
        return base / "ZeroAgents"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "ZeroAgents"
    base = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local" / "state"))
    return base / "zeroagents"


def platform_dir(platform: str) -> Path:
    path = state_dir() / platform
    path.mkdir(parents=True, exist_ok=True)
    return path


def claude_dir() -> Path:
    return Path(os.environ.get("CLAUDE_CONFIG_DIR", Path.home() / ".claude"))
