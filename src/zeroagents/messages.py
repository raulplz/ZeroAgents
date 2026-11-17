"""Mensajes entre agentes: encargos, entregas y mensajes directos.

Se capturan con los mismos hooks que ya instala ZeroAgents (documentados en
https://code.claude.com/docs/en/hooks):

- ENCARGO: `PreToolUse` de la herramienta `Agent` (antes `Task`). Trae el
  `prompt` completo, la `description` y el `subagent_type`.
- ENTREGA:
  - subagente en primer plano: `PostToolUse` de `Agent` con `status: completed`
    y el texto final en `tool_response.content`;
  - subagente en segundo plano: `PostToolUse` devuelve `async_launched` y el
    texto llega después en `SubagentStop.last_assistant_message`;
  - en modo auto, el informe va en `SubagentHandback` (`tool_input.message`) y
    `content` solo trae una nota, así que se usa el handback y no el resultado.
- MENSAJE: `PreToolUse` de `SendMessage` (equipos de agentes y otras sesiones).

`SubagentStop` también salta con agentes internos de Claude Code (sugerencias
de prompt, /btw), por eso solo se usa para los subagentes lanzados en segundo
plano que se han visto antes.

Cada mensaje se guarda en `<sesión>.messages.jsonl` (estado de ZeroAgents, se
borra a los 7 días) y, si el proyecto tiene carpeta `.zeroagents/`, también en
`.zeroagents/mensajes.jsonl`, que es permanente y sirve de evidencia.
"""

from __future__ import annotations

import json
import os
import sys
import time
from datetime import datetime
from pathlib import Path

from zeroagents import store
from zeroagents.paths import platform_dir

PLATFORM = "claude-code"
AGENT_TOOLS = {"Agent", "Task"}
MAX_TEXT = 50_000
PROJECT_LOG = Path(".zeroagents") / "mensajes.jsonl"
MAIN = "principal"

KIND_LABEL = {"encargo": "ENCARGO", "entrega": "ENTREGA", "mensaje": "MENSAJE"}


# ── Captura (la llama el hook; debe ser rápida y no fallar) ─────────────────


def _text(value) -> str:
    """Texto plano de un string, una lista de bloques o un objeto."""
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        parts = []
        for block in value:
            if isinstance(block, dict) and block.get("type", "text") == "text":
                parts.append(str(block.get("text", "")))
            elif isinstance(block, str):
                parts.append(block)
        return "\n".join(p for p in parts if p)
    if isinstance(value, dict):
        for key in ("text", "message", "content"):
            if key in value:
                return _text(value[key])
    return json.dumps(value, ensure_ascii=False)


def _cut(text: str) -> str:
    return text if len(text) <= MAX_TEXT else text[:MAX_TEXT] + "\n[… recortado por ZeroAgents]"


def _who(data: dict) -> str:
    """Quién emite: el subagente o agente con nombre, o la sesión principal."""
    return data.get("agent_type") or MAIN


class Ledger:
    """Recuerda, por sesión, qué subagentes van en segundo plano y cuáles ya
    entregaron por handback, para no duplicar ni confundir agentes internos."""

    def __init__(self, path: Path):
        self.path = path
        data = store.read_json(path) or {}
        self.background = set(data.get("bg", []))
        self.handed = set(data.get("handed", []))

    def _apply(self, change) -> None:
        """Los hooks son asíncronos y pueden coincidir: se relee bajo bloqueo y
        se aplica solo este cambio, para no pisar el de otro proceso."""
        def edit(data: dict) -> None:
            bg, handed = set(data.get("bg", [])), set(data.get("handed", []))
            change(bg, handed)
            data["bg"], data["handed"] = sorted(bg), sorted(handed)
            self.background, self.handed = bg, handed

        store.update_json(self.path, edit)

    def add_background(self, aid: str) -> None:
        self._apply(lambda bg, handed: bg.add(aid))

    def drop_background(self, aid: str) -> None:
        self._apply(lambda bg, handed: bg.discard(aid))

    def add_handed(self, aid: str) -> None:
        self._apply(lambda bg, handed: handed.add(aid))


def message_from_hook(data: dict, ledger: Ledger, now: float | None = None) -> dict | None:
    """Convierte la entrada de un hook en un mensaje, o None si no lo es."""
    event = data.get("hook_event_name")
    tool = data.get("tool_name")
    ti = data.get("tool_input") if isinstance(data.get("tool_input"), dict) else {}
    msg: dict | None = None

    if event == "PreToolUse" and tool in AGENT_TOOLS:
        msg = {"kind": "encargo", "de": _who(data), "para": ti.get("subagent_type") or "general-purpose",
               "titulo": ti.get("description"), "texto": _text(ti.get("prompt")),
               "tool_use_id": data.get("tool_use_id")}

    elif event == "PreToolUse" and tool == "SendMessage":
        dest = ti.get("to") or ti.get("recipient") or ti.get("name") or ti.get("target") or "?"
        msg = {"kind": "mensaje", "de": _who(data), "para": _text(dest),
               "titulo": ti.get("summary"), "texto": _text(ti.get("message") or ti.get("content"))}

    elif event == "PreToolUse" and tool == "SubagentHandback":
        aid = data.get("agent_id")
        if aid:
            ledger.add_handed(aid)
        msg = {"kind": "entrega", "de": data.get("agent_type") or "subagente", "para": MAIN,
               "texto": _text(ti.get("message")), "agent_id": aid, "via": "handback"}

    elif event == "PostToolUse" and tool in AGENT_TOOLS:
        resp = data.get("tool_response") if isinstance(data.get("tool_response"), dict) else {}
        aid = resp.get("agentId")
        if resp.get("status") == "async_launched":
            if aid:
                ledger.add_background(aid)
            return None
        if aid and aid in ledger.handed:
            return None
        text = _text(resp.get("content")) if resp else _text(data.get("tool_response"))
        msg = {"kind": "entrega", "de": ti.get("subagent_type") or "general-purpose", "para": _who(data),
               "titulo": ti.get("description"), "texto": text, "agent_id": aid,
               "tool_use_id": data.get("tool_use_id"), "via": "resultado"}

    elif event == "SubagentStop":
        aid = data.get("agent_id")
        if not aid or aid not in ledger.background:
            return None
        ledger.drop_background(aid)
        if aid in ledger.handed:
            return None
        msg = {"kind": "entrega", "de": data.get("agent_type") or "subagente", "para": MAIN,
               "texto": _text(data.get("last_assistant_message")), "agent_id": aid, "via": "segundo plano"}

    if msg is None or not msg.get("texto"):
        return None
    msg["texto"] = _cut(msg["texto"])
    msg = {"t": now or time.time(), "sesion": data.get("session_id"), **{k: v for k, v in msg.items() if v}}
    return msg


def project_log(data: dict) -> Path | None:
    root = os.environ.get("CLAUDE_PROJECT_DIR") or data.get("cwd")
    if not root:
        return None
    folder = Path(root) / PROJECT_LOG.parent
    return folder / PROJECT_LOG.name if folder.is_dir() else None


def ingest(data: dict) -> dict | None:
    sid = data.get("session_id")
    if not sid:
        return None
    if data.get("hook_event_name") not in ("PreToolUse", "PostToolUse", "SubagentStop"):
        return None
    directory = platform_dir(PLATFORM)
    msg = message_from_hook(data, Ledger(directory / f"{sid}.agents.json"))
    if msg is None:
        return None
    store.append_event(directory / f"{sid}.messages.jsonl", msg)
    if (log := project_log(data)) is not None:
        store.append_event(log, msg)
    return msg


# ── Lectura y formato ───────────────────────────────────────────────────────

COLORS = {"encargo": "\033[38;2;117;196;232m", "entrega": "\033[38;2;77;163;255m",
          "mensaje": "\033[38;2;230;191;121m"}
DIM, BOLD, RESET = "\033[38;2;83;100;119m", "\033[1m", "\033[0m"


def sources(project: Path | None, session: str | None) -> list[Path]:
    """Archivos de los que leer: el registro del proyecto o el estado propio."""
    if project is not None:
        return [project / PROJECT_LOG]
    directory = platform_dir(PLATFORM)
    if session:
        return sorted(directory.glob(f"{session}*.messages.jsonl"))
    return sorted(directory.glob("*.messages.jsonl"))


def load(paths: list[Path], since: float | None = None) -> list[dict]:
    out = []
    for path in paths:
        out.extend(store.JsonlTail(path).read_new())
    if since is not None:
        out = [m for m in out if m.get("t", 0) >= since]
    return sorted(out, key=lambda m: m.get("t", 0))


def format_message(m: dict, full: bool = False, color: bool = True, lines: int = 6,
                   hint: str = "--full para verlo entero") -> str:
    c = (lambda code, s: f"{code}{s}{RESET}") if color else (lambda code, s: s)
    kind = m.get("kind", "mensaje")
    when = datetime.fromtimestamp(m.get("t", 0)).strftime("%H:%M:%S")
    head = f"{c(DIM, when)} {c(COLORS.get(kind, '') + BOLD, KIND_LABEL.get(kind, kind.upper()))} " \
           f"{m.get('de', '?')} → {m.get('para', '?')}"
    if m.get("titulo"):
        head += c(DIM, f" · {m['titulo']}")
    body = m.get("texto", "").rstrip().splitlines()
    if not full and len(body) > lines:
        body = body[:lines] + [f"… ({len(body) - lines} líneas más; {hint})"]
    return head + "\n" + "\n".join(f"  {c(DIM, '│')} {line}" for line in body)


def to_markdown(msgs: list[dict]) -> str:
    out = ["# Registro de mensajes entre agentes", "",
           "Capturado automáticamente por ZeroAgents desde los hooks de Claude Code. "
           "Los textos son íntegros, tal y como se enviaron.", ""]
    for i, m in enumerate(msgs, 1):
        when = datetime.fromtimestamp(m.get("t", 0)).strftime("%d/%m/%Y %H:%M:%S")
        title = f" · {m['titulo']}" if m.get("titulo") else ""
        out += [f"## {i}. {KIND_LABEL.get(m.get('kind'), '?')} · {m.get('de')} → {m.get('para')}{title}", "",
                f"*{when}*", "", "```text", m.get("texto", "").rstrip(), "```", ""]
    return "\n".join(out)


def follow(paths_fn, full: bool, color: bool, interval: float = 0.5) -> None:
    """Muestra los mensajes nuevos según llegan (Ctrl+C para salir)."""
    tails: dict[Path, store.JsonlTail] = {}
    first = True
    try:
        while True:
            new = []
            for path in paths_fn():
                if path not in tails:
                    tails[path] = store.JsonlTail(path)
                    if first:   # al arrancar, solo lo que llegue a partir de ahora
                        tails[path].read_new()
                        continue
                new.extend(tails[path].read_new())
            first = False
            for m in sorted(new, key=lambda m: m.get("t", 0)):
                print(format_message(m, full=full, color=color), flush=True)
            time.sleep(interval)
    except KeyboardInterrupt:
        pass


def use_color() -> bool:
    return sys.stdout.isatty() and not os.environ.get("NO_COLOR")
