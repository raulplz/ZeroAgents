"""Conecta ZeroAgents con Claude Code editando ~/.claude/settings.json.

- Añade hooks asíncronos (no bloquean ni ralentizan a Claude).
- Envuelve tu statusline: se sigue viendo igual y además se guarda una
  instantánea. La original queda en install.json para restaurarla.
Siempre guarda una copia de settings.json antes de tocarlo.
"""

from __future__ import annotations

import json
import shlex
import shutil
import sys
import time
from pathlib import Path

from zeroagents import store
from zeroagents.paths import claude_dir, state_dir

HOOK_EVENTS = [
    "SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "PostToolUseFailure", "PermissionRequest", "Notification", "Stop",
    "SubagentStart", "SubagentStop",
]
HOOK_MARK = " hook claude-code"
STATUS_MARK = " statusline"


def executable() -> str:
    found = shutil.which("zeroagents")
    if found:
        return shlex.quote(found)
    return f"{shlex.quote(sys.executable)} -m zeroagents"


def _settings_path() -> Path:
    return claude_dir() / "settings.json"


def _is_ours(command: str, mark: str) -> bool:
    return "zeroagents" in command and mark in command


def _load(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def _save(path: Path, data: dict) -> None:
    backups = state_dir() / "backups"
    backups.mkdir(parents=True, exist_ok=True)
    if path.exists():
        shutil.copy2(path, backups / f"settings-{time.strftime('%Y%m%d-%H%M%S')}.json")
    store.write_atomic(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def _strip_hooks(settings: dict) -> None:
    hooks = settings.get("hooks") or {}
    for event in list(hooks):
        groups = [
            g for g in hooks[event]
            if not any(_is_ours(h.get("command", ""), HOOK_MARK) for h in g.get("hooks", []))
        ]
        if groups:
            hooks[event] = groups
        else:
            del hooks[event]
    if "hooks" in settings and not hooks:
        del settings["hooks"]


def install(notify: bool = True, statusline: bool = True) -> list[str]:
    with store.locked(state_dir() / "install"):
        return _install(notify, statusline)


def _install(notify: bool, statusline: bool) -> list[str]:
    path = _settings_path()
    settings = _load(path)
    exe = executable()
    notes = []

    _strip_hooks(settings)
    command = f"{exe}{HOOK_MARK}" + ("" if notify else " --no-notify")
    hooks = settings.setdefault("hooks", {})
    for event in HOOK_EVENTS:
        hooks.setdefault(event, []).append(
            {"hooks": [{"type": "command", "command": command, "async": True, "timeout": 10}]}
        )
    notes.append(f"Hooks asíncronos en {len(HOOK_EVENTS)} eventos.")

    if statusline:
        current = settings.get("statusLine") or {}
        if _is_ours(current.get("command", ""), STATUS_MARK):
            notes.append("La statusline ya pasaba por ZeroAgents.")
        else:
            record = state_dir() / "install.json"
            record.parent.mkdir(parents=True, exist_ok=True)
            store.write_atomic(record, json.dumps({"original_statusLine": current or None}, indent=2))
            wrapped = f"{exe}{STATUS_MARK}"
            if current.get("command"):
                wrapped += f" --chain {shlex.quote(current['command'])}"
            settings["statusLine"] = {**current, "type": "command", "command": wrapped}
            notes.append("Statusline envuelta (se ve igual que antes).")

    _save(path, settings)
    notes.append(f"Copia previa de settings.json en {state_dir() / 'backups'}")
    return notes


def uninstall() -> list[str]:
    with store.locked(state_dir() / "install"):
        return _uninstall()


def _uninstall() -> list[str]:
    path = _settings_path()
    settings = _load(path)
    _strip_hooks(settings)
    notes = ["Hooks de ZeroAgents eliminados."]
    current = settings.get("statusLine") or {}
    if _is_ours(current.get("command", ""), STATUS_MARK):
        record = state_dir() / "install.json"
        original = (json.loads(record.read_text()) if record.exists() else {}).get("original_statusLine")
        if original:
            settings["statusLine"] = original
            notes.append("Statusline original restaurada.")
        else:
            settings.pop("statusLine", None)
            notes.append("Statusline de ZeroAgents eliminada.")
    _save(path, settings)
    return notes


def status() -> dict:
    settings = _load(_settings_path())
    hooks = settings.get("hooks") or {}
    hooked = [
        e for e in HOOK_EVENTS
        if any(_is_ours(h.get("command", ""), HOOK_MARK) for g in hooks.get(e, []) for h in g.get("hooks", []))
    ]
    sl = (settings.get("statusLine") or {}).get("command", "")
    return {"hooks": hooked, "missing_hooks": [e for e in HOOK_EVENTS if e not in hooked],
            "statusline": _is_ours(sl, STATUS_MARK)}
