"""Adaptador de Claude Code.

Fuentes, de más a menos fiable:
- Hooks (documentados): modo de permisos, esfuerzo y, sobre todo, si el agente
  espera un permiso o una respuesta tuya. `zeroagents hook claude-code` solo
  apunta el evento en `<sesión>.events.jsonl`.
- Statusline (documentada): modelo, esfuerzo, coste, contexto y límites de la
  cuenta. `zeroagents statusline` guarda una instantánea y pinta tu barra.
- `~/.claude/sessions/<pid>.json` (interno, puede cambiar): qué sesiones están
  vivas, su nombre y si están ocupadas o libres.
- Transcript `.jsonl` (interno): tokens acumulados y respaldo de modelo, modo
  y esfuerzo para sesiones abiertas antes de instalar ZeroAgents.
"""

from __future__ import annotations

import os
import re
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

from zeroagents import store
from zeroagents.adapters.base import Adapter
from zeroagents.model import Agent, Limits, Status, Usage
from zeroagents.paths import claude_dir, platform_dir

PLATFORM = "claude-code"

QUESTION_TOOLS = {"AskUserQuestion", "ExitPlanMode"}
QUESTION_NOTIFICATIONS = {"elicitation_dialog", "elicitation_url_dialog", "agent_needs_input"}
NOTIFY_TYPES = {"permission_prompt", "idle_prompt"} | QUESTION_NOTIFICATIONS

# Si el registro dice «libre» bastante después de una petición de permiso,
# la petición se respondió o se canceló (Esc no dispara ningún hook).
REGISTRY_MARGIN_S = 2.0
ENDED_WINDOW_S = 24 * 3600


def _clip(text: str | None, limit: int) -> str | None:
    if not text:
        return None
    text = " ".join(str(text).split())
    return text if len(text) <= limit else text[: limit - 1] + "…"


def _describe_tool(tool: str | None, tool_input: dict | None) -> str:
    tool_input = tool_input or {}
    if tool == "AskUserQuestion":
        questions = tool_input.get("questions") or []
        if questions and isinstance(questions[0], dict):
            return questions[0].get("question") or "Te ha hecho una pregunta"
        return "Te ha hecho una pregunta"
    if tool == "ExitPlanMode":
        return "Tiene un plan listo para que lo apruebes"
    detail = (
        tool_input.get("description")
        or tool_input.get("command")
        or tool_input.get("file_path")
        or tool_input.get("url")
        or tool_input.get("pattern")
        or ""
    )
    return f"{tool}: {detail}" if detail else f"Quiere usar {tool}"


# ── Ingesta (la ejecutan los hooks y la statusline; debe ser rápida) ────────


def event_from_hook(data: dict, now: float | None = None) -> dict:
    """Reduce la entrada de un hook a lo que necesita el panel."""
    name = data.get("hook_event_name", "")
    ev: dict = {"t": now or time.time(), "e": name}
    effort = data.get("effort")
    simple = {
        "pm": data.get("permission_mode"),
        "eff": effort.get("level") if isinstance(effort, dict) else None,
        "ag": data.get("agent_id"),
        "model": data.get("model") if isinstance(data.get("model"), str) else None,
        "st": data.get("session_title"),
        "cwd": data.get("cwd"),
        "tp": data.get("transcript_path"),
        "src": data.get("source"),
        "nt": data.get("notification_type"),
    }
    ev.update({k: v for k, v in simple.items() if v})
    if name == "Notification":
        ev["msg"] = _clip(data.get("message"), 300)
    elif name in ("PreToolUse", "PermissionRequest"):
        tool = data.get("tool_name")
        ev["tool"] = tool
        if name == "PermissionRequest" or tool in QUESTION_TOOLS:
            ev["msg"] = _clip(_describe_tool(tool, data.get("tool_input")), 300)
    elif name in ("PostToolUse", "PostToolUseFailure"):
        ev["tool"] = data.get("tool_name")
    elif name == "Stop":
        ev["last"] = _clip(data.get("last_assistant_message"), 600)
    return ev


def ingest_hook(data: dict, notify: bool = True) -> None:
    sid = data.get("session_id")
    if not sid:
        return
    ev = event_from_hook(data)
    store.append_event(platform_dir(PLATFORM) / f"{sid}.events.jsonl", ev)
    if ev["e"] in ("PreToolUse", "PostToolUse", "SubagentStop"):
        from zeroagents import messages

        messages.ingest(data)
    if notify and ev["e"] == "Notification" and ev.get("nt") in NOTIFY_TYPES:
        from zeroagents.notify import send

        name = _registry_name(sid) or Path(data.get("cwd") or "").name or sid[:8]
        send(f"ZeroAgents · {name}", ev.get("msg") or "Te necesita")


def ingest_statusline(data: dict) -> None:
    sid = data.get("session_id")
    if not sid:
        return
    model = data.get("model") or {}
    cost = data.get("cost") or {}
    ctx = data.get("context_window") or {}
    snap = {
        "t": time.time(),
        "model_id": model.get("id"),
        "model_name": model.get("display_name"),
        "effort": (data.get("effort") or {}).get("level"),
        "fast": bool(data.get("fast_mode")),
        "cost": cost.get("total_cost_usd"),
        "duration_ms": cost.get("total_duration_ms"),
        "ctx_pct": ctx.get("used_percentage"),
        "ctx_size": ctx.get("context_window_size"),
        "session_name": data.get("session_name"),
        "cwd": (data.get("workspace") or {}).get("current_dir") or data.get("cwd"),
        "tp": data.get("transcript_path"),
        "rate_limits": data.get("rate_limits"),
    }
    store.write_snapshot(platform_dir(PLATFORM) / f"{sid}.status.json", snap)


def _registry_name(sid: str) -> str | None:
    for entry in read_registry().values():
        if entry.get("sessionId") == sid:
            return entry.get("name")
    return None


# ── Plegado de eventos ──────────────────────────────────────────────────────


@dataclass
class HookState:
    pending: dict = field(default_factory=dict)  # clave -> (tipo, mensaje, t)
    last_main: str | None = None
    last_main_t: float = 0.0
    last_t: float = 0.0
    idle_notified: bool = False
    subagents: set = field(default_factory=set)
    values: dict = field(default_factory=dict)   # campo -> (valor, t)
    last_message: str | None = None

    def remember(self, key: str, value, t: float) -> None:
        if value and t >= self.values.get(key, (None, 0.0))[1]:
            self.values[key] = (value, t)

    def apply(self, ev: dict) -> None:
        e, t = ev.get("e"), ev.get("t", 0.0)
        key = ev.get("ag") or "main"
        self.last_t = max(self.last_t, t)
        for src, dst in (("pm", "mode"), ("eff", "effort"), ("model", "model"),
                         ("st", "title"), ("cwd", "cwd"), ("tp", "tp")):
            self.remember(dst, ev.get(src), t)

        if e == "Notification":
            nt = ev.get("nt")
            msg = ev.get("msg")
            if nt == "permission_prompt":
                kinds = [p[0] for p in self.pending.values()]
                if "permission" not in kinds:
                    self.pending["notif"] = ("permission", msg, t)
            elif nt in QUESTION_NOTIFICATIONS:
                self.pending["notif"] = ("question", msg, t)
            elif nt in ("elicitation_complete", "elicitation_response"):
                self.pending.pop("notif", None)
            elif nt == "idle_prompt":
                self.idle_notified = True
            return

        # Cualquier otro evento significa que algo avanzó.
        self.pending.pop("notif", None)
        self.pending.pop(key, None)
        if e == "PermissionRequest":
            self.pending[key] = ("permission", ev.get("msg"), t)
        elif e == "PreToolUse" and ev.get("tool") in QUESTION_TOOLS:
            self.pending[key] = ("question", ev.get("msg"), t)
        elif e == "SubagentStart" and ev.get("ag"):
            self.subagents.add(ev["ag"])
        elif e == "SubagentStop":
            self.subagents.discard(ev.get("ag"))
        elif e in ("UserPromptSubmit", "Stop", "SessionStart", "SessionEnd") and key == "main":
            self.pending.clear()
            if e in ("SessionStart", "SessionEnd"):
                self.subagents.clear()

        if key == "main" and e not in ("SubagentStart", "SubagentStop"):
            self.last_main, self.last_main_t = e, t
            self.idle_notified = False
            if e == "Stop" and ev.get("last"):
                self.last_message = ev["last"]

    def value(self, key: str):
        return self.values.get(key, (None, 0.0))

    def status(self) -> tuple[Status, str | None, float]:
        """Estado según los hooks: (estado, qué necesita, desde cuándo)."""
        for kind, status in (("permission", Status.PERMISSION), ("question", Status.QUESTION)):
            hits = [p for p in self.pending.values() if p[0] == kind]
            if hits:
                newest = max(hits, key=lambda p: p[2])
                return status, newest[1], newest[2]
        if self.last_main == "SessionEnd":
            return Status.ENDED, None, self.last_main_t
        if self.idle_notified or self.last_main in ("Stop", "SessionStart"):
            return Status.YOUR_TURN, None, self.last_main_t
        if self.last_main:
            return Status.WORKING, None, self.last_main_t
        return Status.UNKNOWN, None, 0.0


def fold(events: list[dict], state: HookState | None = None) -> HookState:
    state = state or HookState()
    for ev in sorted(events, key=lambda e: e.get("t", 0.0)):
        state.apply(ev)
    return state


# ── Transcript ──────────────────────────────────────────────────────────────


def _iso(ts: str | None) -> float:
    if not ts:
        return 0.0
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return 0.0


@dataclass
class TranscriptStats:
    values: dict = field(default_factory=dict)
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    requests: int = 0
    seen: set = field(default_factory=set)
    last_t: float = 0.0

    def remember(self, key: str, value, t: float) -> None:
        if value and t >= self.values.get(key, (None, 0.0))[1]:
            self.values[key] = (value, t)

    def apply(self, d: dict) -> None:
        kind = d.get("type")
        t = _iso(d.get("timestamp"))
        self.last_t = max(self.last_t, t)
        if kind == "assistant":
            msg = d.get("message") or {}
            model = msg.get("model")
            if model and not model.startswith("<"):
                self.remember("model", model, t)
            self.remember("effort", d.get("effort"), t)
            usage = msg.get("usage")
            rid = d.get("requestId") or msg.get("id")
            # Una respuesta puede ocupar varias líneas con el mismo uso.
            if usage and rid not in self.seen:
                self.seen.add(rid)
                self.requests += 1
                self.input_tokens += (usage.get("input_tokens") or 0) + (
                    usage.get("cache_creation_input_tokens") or 0
                )
                self.cache_read_tokens += usage.get("cache_read_input_tokens") or 0
                self.output_tokens += usage.get("output_tokens") or 0
        elif kind == "user":
            self.remember("mode", d.get("permissionMode"), t)
        elif kind == "ai-title":
            self.remember("title", d.get("aiTitle"), t or self.last_t)
        elif kind == "custom-title":
            self.remember("custom_title", d.get("customTitle"), t or self.last_t)


def transcript_path_for(cwd: str, sid: str) -> Path:
    guess = claude_dir() / "projects" / re.sub(r"[^A-Za-z0-9]", "-", cwd) / f"{sid}.jsonl"
    if guess.exists():
        return guess
    found = next((claude_dir() / "projects").glob(f"*/{sid}.jsonl"), None)
    return found or guess


# ── Registro de sesiones vivas ──────────────────────────────────────────────


def read_registry() -> dict[str, dict]:
    out = {}
    for path in (claude_dir() / "sessions").glob("*.json"):
        entry = store.read_json(path)
        if entry and entry.get("sessionId"):
            out[entry["sessionId"]] = entry
    return out


def pid_alive(pid: int | None, proc_start: str | None = None) -> bool:
    if not pid:
        return False
    if sys.platform.startswith("linux"):
        try:
            stat = Path(f"/proc/{pid}/stat").read_text()
        except OSError:
            return False
        if proc_start:
            # Campo 22 (starttime): evita confundir un PID reutilizado.
            fields = stat.rsplit(")", 1)[1].split()
            return fields[19] == str(proc_start)
        return True
    if sys.platform == "win32":
        return True  # os.kill en Windows mataría el proceso; nos fiamos del registro
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


# ── Adaptador ───────────────────────────────────────────────────────────────


def pretty_model(model_id: str | None) -> str | None:
    """claude-opus-5-5 -> Opus 5.5; claude-sonnet-5-5[1m] -> Sonnet 5.5 · 1M"""
    if not model_id:
        return None
    big = "[1m]" in model_id
    core = model_id.replace("[1m]", "").removeprefix("claude-")
    parts = [p for p in core.split("-") if not (p.isdigit() and len(p) == 8)]
    if not parts or not parts[0].isalpha():
        return model_id
    family = parts[0].capitalize()
    version = ".".join(p for p in parts[1:] if p.isdigit())
    name = f"{family} {version}".strip()
    return f"{name} · 1M" if big else name


class ClaudeCodeAdapter(Adapter):
    platform = PLATFORM
    label = "Claude Code"

    def __init__(self) -> None:
        self.dir = platform_dir(PLATFORM)
        self._events: dict[str, tuple[store.JsonlTail, HookState]] = {}
        self._transcripts: dict[str, tuple[store.JsonlTail, TranscriptStats]] = {}
        store.prune(self.dir)

    def _hook_state(self, sid: str) -> HookState:
        if sid not in self._events:
            self._events[sid] = (store.JsonlTail(self.dir / f"{sid}.events.jsonl"), HookState())
        tail, state = self._events[sid]
        return fold(tail.read_new(), state)

    def _transcript(self, path: Path) -> TranscriptStats:
        key = str(path)
        if key not in self._transcripts:
            self._transcripts[key] = (store.JsonlTail(path), TranscriptStats())
        tail, stats = self._transcripts[key]
        for d in tail.read_new():
            stats.apply(d)
        return stats

    def _known_sessions(self) -> set[str]:
        limit = time.time() - ENDED_WINDOW_S
        sids = set()
        for path in self.dir.glob("*.json*"):
            try:
                if path.stat().st_mtime >= limit:
                    sids.add(path.name.split(".", 1)[0])
            except OSError:
                pass
        return sids

    def collect(self, include_ended: bool = False) -> tuple[list[Agent], Limits | None]:
        registry = read_registry()
        live = {
            sid: entry for sid, entry in registry.items()
            if pid_alive(entry.get("pid"), entry.get("procStart"))
        }
        sids = set(live) | (self._known_sessions() if include_ended else set())
        agents, limits = [], None
        for sid in sids:
            agent, rate = self._build(sid, live.get(sid))
            if agent.alive or include_ended:
                agents.append(agent)
            if rate and (limits is None or (rate.observed_at or 0) > (limits.observed_at or 0)):
                limits = rate
        return agents, limits

    def _build(self, sid: str, reg: dict | None) -> tuple[Agent, Limits | None]:
        hooks = self._hook_state(sid)
        snap = store.read_json(self.dir / f"{sid}.status.json") or {}
        snap_t = snap.get("t", 0.0)
        cwd = (reg or {}).get("cwd") or snap.get("cwd") or hooks.value("cwd")[0] or ""
        tp = snap.get("tp") or hooks.value("tp")[0]
        tpath = Path(tp) if tp else transcript_path_for(cwd, sid)
        tx = self._transcript(tpath)

        def latest(key: str, *extra: tuple):
            options = [hooks.value(key), tx.values.get(key, (None, 0.0)), *extra]
            options = [o for o in options if o[0]]
            return max(options, key=lambda o: o[1])[0] if options else None

        model = latest("model", (snap.get("model_id"), snap_t))
        effort = latest("effort", (snap.get("effort"), snap_t))
        mode = latest("mode")

        status, need, since = hooks.status()
        alive = reg is not None
        if reg:
            reg_t = (reg.get("statusUpdatedAt") or 0) / 1000
            reg_status = {"busy": Status.WORKING, "idle": Status.YOUR_TURN}.get(reg.get("status"))
            if status.needs_you:
                if reg_status is Status.YOUR_TURN and reg_t > since + REGISTRY_MARGIN_S:
                    status, need = Status.YOUR_TURN, None
            elif reg_status and (reg_t > hooks.last_t or status in (Status.UNKNOWN, Status.ENDED)):
                status = reg_status
        else:
            status, need = Status.ENDED, None

        if reg and reg.get("nameSource") == "user":
            name = reg.get("name")
        else:
            name = (
                snap.get("session_name")
                or latest("custom_title")
                or latest("title")
                or (reg or {}).get("name")
            )
        usage = Usage(
            cost_usd=snap.get("cost"),
            context_pct=snap.get("ctx_pct"),
            context_size=snap.get("ctx_size"),
            input_tokens=tx.input_tokens,
            output_tokens=tx.output_tokens,
            cache_read_tokens=tx.cache_read_tokens,
            requests=tx.requests,
            duration_ms=snap.get("duration_ms"),
        )
        agent = Agent(
            platform=PLATFORM,
            id=sid,
            name=name or sid[:8],
            cwd=cwd,
            status=status,
            model=model,
            effort=effort,
            mode=mode,
            fast=bool(snap.get("fast")),
            usage=usage,
            attention_msg=need,
            last_message=hooks.last_message,
            last_activity=max(hooks.last_t, tx.last_t, snap_t, ((reg or {}).get("updatedAt") or 0) / 1000) or None,
            subagents=len(hooks.subagents),
            pid=(reg or {}).get("pid"),
            kind=(reg or {}).get("kind"),
            alive=alive,
            transcript=str(tpath),
        )
        return agent, _limits(snap)


def _limits(snap: dict) -> Limits | None:
    rate = snap.get("rate_limits")
    if not rate:
        return None
    five, seven = rate.get("five_hour") or {}, rate.get("seven_day") or {}
    return Limits(
        five_hour_pct=five.get("used_percentage"),
        five_hour_resets=five.get("resets_at"),
        seven_day_pct=seven.get("used_percentage"),
        seven_day_resets=seven.get("resets_at"),
        observed_at=snap.get("t"),
    )
