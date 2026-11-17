"""El plan de equipo: esquema JSON para el arquitecto, validación y lectura/escritura."""

from __future__ import annotations

import json
import re
from pathlib import Path

from zeroagents import store

PHASES = ("requisitos", "arquitectura", "implementacion", "qa", "transversal")
PHASE_LABELS = {
    "requisitos": "1 · Requisitos",
    "arquitectura": "2 · Arquitectura",
    "implementacion": "3 · Implementación",
    "qa": "4 · Pruebas QA",
    "transversal": "Transversal",
}
EFFORTS = ("low", "medium", "high", "xhigh", "max")
DEFAULT_MODELS = ("haiku", "sonnet", "opus")
ALL_MODELS = ("haiku", "sonnet", "opus", "fable")
TOOLS = ("Read", "Write", "Edit", "Glob", "Grep", "Bash", "WebFetch", "WebSearch")
COLORS = ("blue", "cyan", "green", "yellow", "purple", "orange", "pink", "red")
RESERVED = {"team-lead", "lead", "lider", "líder"}
NAME_RE = re.compile(r"^[a-z][a-z0-9-]{1,39}$")

PLAN_FILE = Path(".zeroagents") / "team.json"

LEAD_ROLE = "Product Owner"
LEAD_ROLE_DESC = ("dueño del backlog: prioriza por valor, define y comprueba los criterios de aceptación, "
                  "acepta o rechaza cada entrega y coordina al equipo")


def _strs(desc: str) -> dict:
    return {"type": "array", "items": {"type": "string"}, "description": desc}


def schema(models: tuple[str, ...] = DEFAULT_MODELS) -> dict:
    """Esquema que `claude -p --json-schema` impone a la respuesta del arquitecto."""
    obj = lambda props: {  # noqa: E731
        "type": "object", "properties": props,
        "required": list(props), "additionalProperties": False,
    }
    model = {"type": "string", "enum": list(models)}
    effort = {"type": "string", "enum": list(EFFORTS)}
    return obj({
        "project": obj({
            "name": {"type": "string", "description": "Nombre corto del proyecto"},
            "slug": {"type": "string", "description": "kebab-case, para la carpeta"},
            "summary": {"type": "string", "description": "Qué es y qué problema resuelve (2-3 frases)"},
            "goal": {"type": "string", "description": "Resultado concreto que se entrega"},
            "users": _strs("Tipos de usuario"),
            "stack": _strs("Tecnologías elegidas"),
            "in_scope": _strs("Qué entra"),
            "out_of_scope": _strs("Qué queda fuera a propósito"),
            "assumptions": _strs("Suposiciones que haces"),
            "open_questions": _strs("Dudas que el humano debería resolver"),
        }),
        "lead": obj({
            "model": model, "effort": effort,
            "why": {"type": "string", "description": "Por qué ese modelo y esfuerzo para coordinar"},
        }),
        "agents": {"type": "array", "minItems": 2, "items": obj({
            "name": {"type": "string", "description": "kebab-case único, p. ej. dev-backend"},
            "title": {"type": "string", "description": "Rol legible, p. ej. Desarrollador backend"},
            "phase": {"type": "string", "enum": list(PHASES)},
            "model": model, "effort": effort,
            "why": {"type": "string", "description": "Por qué ese modelo y ese esfuerzo"},
            "description": {"type": "string", "description": "Cuándo usar este agente (una frase)"},
            "tools": {"type": "array", "items": {"type": "string", "enum": list(TOOLS)}},
            "inputs": _strs("Artefactos que lee"),
            "outputs": _strs("Artefactos que escribe y de los que es dueño"),
            "system_prompt": {"type": "string", "description": "System prompt completo del agente"},
        })},
        "tasks": {"type": "array", "minItems": 1, "items": obj({
            "id": {"type": "string", "description": "T01, T02…"},
            "subject": {"type": "string"},
            "description": {"type": "string"},
            "owner": {"type": "string", "description": "name de un agente"},
            "phase": {"type": "string", "enum": list(PHASES)},
            "blocked_by": _strs("ids de tareas que deben acabar antes"),
            "done_when": {"type": "string", "description": "Criterio comprobable de terminado"},
        })},
        "gates": {"type": "array", "items": obj({
            "after_phase": {"type": "string", "enum": list(PHASES)},
            "condition": {"type": "string"},
            "human_approval": {"type": "boolean"},
        })},
    })


def validate(plan: dict, models: tuple[str, ...] = ALL_MODELS) -> list[str]:
    """Comprobaciones que el esquema JSON no puede hacer. Devuelve los errores."""
    errors: list[str] = []
    agents = plan.get("agents") or []
    names = [a.get("name", "") for a in agents]
    for name in names:
        if not NAME_RE.match(name):
            errors.append(f"nombre de agente no válido: {name!r} (kebab-case, 2-40 caracteres)")
        if name in RESERVED:
            errors.append(f"nombre reservado para el líder: {name!r}")
    for dup in {n for n in names if names.count(n) > 1}:
        errors.append(f"agente repetido: {dup}")
    for a in agents:
        if a.get("model") not in models:
            errors.append(f"{a.get('name')}: modelo no permitido {a.get('model')!r}")
        if a.get("effort") not in EFFORTS:
            errors.append(f"{a.get('name')}: esfuerzo no válido {a.get('effort')!r}")
        if not (a.get("system_prompt") or "").strip():
            errors.append(f"{a.get('name')}: falta el system prompt")

    tasks = plan.get("tasks") or []
    ids = [t.get("id", "") for t in tasks]
    for dup in {i for i in ids if ids.count(i) > 1}:
        errors.append(f"tarea repetida: {dup}")
    for t in tasks:
        if t.get("owner") not in names:
            errors.append(f"{t.get('id')}: el responsable {t.get('owner')!r} no está en el equipo")
        for dep in t.get("blocked_by") or []:
            if dep not in ids:
                errors.append(f"{t.get('id')}: depende de una tarea que no existe ({dep})")
            if dep == t.get("id"):
                errors.append(f"{t.get('id')}: depende de sí misma")
    if cycle := _find_cycle({t["id"]: list(t.get("blocked_by") or []) for t in tasks if t.get("id")}):
        errors.append("dependencias en ciclo: " + " → ".join(cycle))
    return errors


def _find_cycle(graph: dict[str, list[str]]) -> list[str] | None:
    state: dict[str, int] = {}
    stack: list[str] = []

    def visit(node: str) -> list[str] | None:
        state[node] = 1
        stack.append(node)
        for nxt in graph.get(node, []):
            if nxt not in graph:
                continue
            if state.get(nxt) == 1:
                return stack[stack.index(nxt):] + [nxt]
            if not state.get(nxt) and (found := visit(nxt)):
                return found
        stack.pop()
        state[node] = 2
        return None

    for node in graph:
        if not state.get(node) and (found := visit(node)):
            return found
    return None


def slugify(text: str) -> str:
    import unicodedata

    plain = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode()
    slug = re.sub(r"[^a-z0-9]+", "-", plain.lower()).strip("-")
    return slug[:40].strip("-") or "proyecto"


def save(directory: Path, data: dict) -> Path:
    path = directory / PLAN_FILE
    path.parent.mkdir(parents=True, exist_ok=True)
    store.write_atomic(path, json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    return path


def load(directory: Path) -> dict | None:
    try:
        return json.loads((directory / PLAN_FILE).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def task_order(tasks: list[dict]) -> list[dict]:
    """Orden topológico estable (las dependencias primero)."""
    by_id = {t["id"]: t for t in tasks}
    done: set[str] = set()
    out: list[dict] = []

    def add(t: dict) -> None:
        if t["id"] in done:
            return
        done.add(t["id"])
        for dep in t.get("blocked_by") or []:
            if dep in by_id:
                add(by_id[dep])
        out.append(t)

    for t in tasks:
        add(t)
    return out
