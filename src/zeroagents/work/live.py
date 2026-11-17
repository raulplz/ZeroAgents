"""Estado en vivo de los proyectos del modo Work.

Junta tres fuentes:
- el plan (`.zeroagents/team.json`): roles, modelos y esfuerzos previstos;
- las sesiones de Claude Code que ve el monitor (el líder y si te necesita);
- el estado interno de los equipos de Claude Code: `~/.claude/teams/<equipo>/config.json`
  y `~/.claude/tasks/<equipo>/`. Ese formato no está documentado y puede cambiar, así que
  se lee de forma tolerante y, si no hay nada, se muestran las tareas del plan.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from pathlib import Path

from zeroagents.model import Agent
from zeroagents.paths import claude_dir, state_dir
from zeroagents import store
from zeroagents.store import read_json
from zeroagents.work import plan as P

PENDING, RUNNING, DONE = "pendiente", "en curso", "hecha"
_STATUS = {
    "pending": PENDING, "todo": PENDING, "open": PENDING, "not_started": PENDING,
    "in_progress": RUNNING, "in progress": RUNNING, "in-progress": RUNNING, "running": RUNNING,
    "active": RUNNING, "claimed": RUNNING,
    "completed": DONE, "complete": DONE, "done": DONE, "finished": DONE,
}


# ── registro de proyectos ───────────────────────────────────────────────────

def _registry_path() -> Path:
    path = state_dir() / "work" / "projects.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


REGISTRY_VERSION = 1


def _migrate_registry(data: dict) -> bool:
    """v0 (sin `version`) → v1. Solo añade; no quita nada."""
    if data.get("version", 0) >= REGISTRY_VERSION:
        return False
    data["version"] = REGISTRY_VERSION
    data.setdefault("projects", [])
    return True


def register(directory: Path, name: str) -> None:
    """Apunta el proyecto el primero de la lista. Lectura-modificación-escritura
    bajo bloqueo: la GUI usa el mismo `projects.json.lock`. Conserva los campos
    desconocidos, también los del propio proyecto si ya estaba."""
    key = str(directory)

    def change(data: dict) -> None:
        items = data.get("projects")
        items = items if isinstance(items, list) else []
        previous = next((p for p in items if isinstance(p, dict) and p.get("dir") == key), {})
        rest = [p for p in items if not (isinstance(p, dict) and p.get("dir") == key)]
        data["projects"] = [{**previous, "dir": key, "name": name, "added": time.time()}, *rest]

    store.update_json(_registry_path(), change, migrate=_migrate_registry, indent=2)


def projects() -> list[Path]:
    """Proyectos registrados que siguen teniendo plan, del más reciente al más antiguo."""
    out = []
    for item in (read_json(_registry_path()) or {}).get("projects", []):
        if not isinstance(item, dict) or not isinstance(item.get("dir"), str):
            continue
        directory = Path(item["dir"])
        if (directory / P.PLAN_FILE).exists():
            out.append(directory)
    return out


# ── vistas ──────────────────────────────────────────────────────────────────

@dataclass
class TaskView:
    id: str
    subject: str
    owner: str | None
    status: str
    blocked_by: list[str] = field(default_factory=list)
    blocked: bool = False


@dataclass
class MemberView:
    name: str
    title: str
    phase: str
    model: str
    effort: str
    why: str = ""
    in_team: bool = False          # aparece en el equipo real de Claude Code
    state: str = "sin arrancar"
    current: str | None = None     # tarea en curso


@dataclass
class ProjectView:
    directory: Path
    data: dict
    lead: Agent | None = None      # sesión del líder, si está abierta
    team_name: str | None = None
    members: list[MemberView] = field(default_factory=list)
    tasks: list[TaskView] = field(default_factory=list)
    live_tasks: bool = False       # True si las tareas vienen del equipo real

    @property
    def name(self) -> str:
        return self.data["plan"]["project"]["name"]

    @property
    def progress(self) -> tuple[int, int]:
        return sum(t.status == DONE for t in self.tasks), len(self.tasks)

    @property
    def phase(self) -> str:
        """Fase actual: la primera del ciclo con tareas sin terminar."""
        plan = self.data["plan"]
        by_id = {t["id"]: t["phase"] for t in plan["tasks"]}
        by_subject = {t["subject"].lower(): t["phase"] for t in plan["tasks"]}
        by_owner = {a["name"]: a["phase"] for a in plan["agents"]}

        def phase_of(t: TaskView) -> str | None:
            # El equipo real numera las tareas a su manera: id, luego título, luego responsable.
            return by_id.get(t.id) or by_subject.get(t.subject.lower()) or by_owner.get(t.owner or "")

        for phase in P.PHASES:
            if any(phase_of(t) == phase and t.status != DONE for t in self.tasks):
                return P.PHASE_LABELS[phase]
        return "terminado" if self.tasks and all(t.status == DONE for t in self.tasks) else "—"


# ── lectura tolerante del estado de los equipos ─────────────────────────────

def _first(d: dict, *keys, default=None):
    for k in keys:
        if d.get(k) not in (None, ""):
            return d[k]
    return default


def _norm_task(d: dict) -> TaskView | None:
    subject = _first(d, "subject", "title", "content", "description")
    if not isinstance(subject, str):
        return None
    status = str(_first(d, "status", "state", default="pending")).lower()
    deps = _first(d, "blockedBy", "blocked_by", "dependencies", "depends_on", default=[]) or []
    return TaskView(
        id=str(_first(d, "id", "taskId", "task_id", default="?")),
        subject=subject.strip().splitlines()[0],
        owner=_first(d, "owner", "assignee", "teammate", "assigned_to"),
        status=_STATUS.get(status, status),
        blocked_by=[str(x) for x in deps] if isinstance(deps, list) else [],
    )


def read_team_tasks(team_dir: Path) -> list[TaskView]:
    tasks: list[TaskView] = []
    for path in sorted(team_dir.glob("*.json")):
        raw = read_json(path)
        if raw is None:
            try:
                raw = json.loads(path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                continue
        items = raw if isinstance(raw, list) else raw.get("tasks") if isinstance(raw, dict) and \
            isinstance(raw.get("tasks"), list) else [raw]
        for item in items:
            if isinstance(item, dict) and (task := _norm_task(item)):
                tasks.append(task)
    return tasks


def read_team_members(config: dict) -> dict[str, dict]:
    members = config.get("members") or []
    out = {}
    for m in members if isinstance(members, list) else []:
        if isinstance(m, dict) and (name := _first(m, "name", "agentName")):
            out[str(name)] = m
    return out


def find_team(directory: Path, sessions: list[Agent]) -> str | None:
    """Equipo cuyo líder es una sesión abierta en la carpeta del proyecto."""
    teams = claude_dir() / "teams"
    if not teams.is_dir():
        return None
    here = [a for a in sessions if a.cwd and Path(a.cwd).resolve() == directory.resolve()]
    for agent in sorted(here, key=lambda a: -(a.last_activity or 0)):
        candidate = f"session-{agent.id[:8]}"
        if (teams / candidate).is_dir():
            return candidate
    # Respaldo: un config.json que mencione la carpeta o una sesión de aquí.
    ids = {a.id for a in here}
    for cfg in teams.glob("*/config.json"):
        text = cfg.read_text(encoding="utf-8", errors="ignore")
        if str(directory) in text or any(i in text for i in ids):
            return cfg.parent.name
    return None


def build(directory: Path, sessions: list[Agent]) -> ProjectView | None:
    data = P.load(directory)
    if not data or "plan" not in data:
        return None
    plan = data["plan"]
    view = ProjectView(directory=directory, data=data)

    # Solo sesiones abiertas: la del arquitecto (`claude -p`) también corrió aquí y ya terminó.
    here = [a for a in sessions if a.alive and a.cwd and Path(a.cwd).resolve() == directory.resolve()]
    view.lead = max(here, key=lambda a: a.last_activity or 0, default=None)
    view.team_name = find_team(directory, sessions)

    live_members: dict[str, dict] = {}
    if view.team_name:
        cfg = read_json(claude_dir() / "teams" / view.team_name / "config.json") or {}
        live_members = read_team_members(cfg)
        view.tasks = read_team_tasks(claude_dir() / "tasks" / view.team_name)
        view.live_tasks = bool(view.tasks)
    if not view.tasks:
        view.tasks = [TaskView(t["id"], t["subject"], t["owner"], PENDING, list(t.get("blocked_by") or []))
                      for t in P.task_order(plan["tasks"])]

    done = {t.id for t in view.tasks if t.status == DONE}
    for t in view.tasks:
        t.blocked = t.status == PENDING and any(d not in done for d in t.blocked_by)

    for a in plan["agents"]:
        m = MemberView(a["name"], a["title"], a["phase"], a["model"], a["effort"], a.get("why", ""))
        m.in_team = a["name"] in live_members
        mine = [t for t in view.tasks if t.owner == a["name"]]
        running = next((t for t in mine if t.status == RUNNING), None)
        if running:
            m.state, m.current = "trabajando", running.subject
        elif not view.team_name:
            m.state = "sin arrancar"
        elif mine and all(t.status == DONE for t in mine):
            m.state = "terminado"
        elif any(t.status == PENDING and not t.blocked for t in mine):
            m.state = "con tarea lista"
        elif any(t.blocked for t in mine):
            m.state = "esperando dependencias"
        else:
            m.state = "libre" if m.in_team else "no creado"
        view.members.append(m)
    return view
