"""Convierte un plan en archivos del proyecto. Nunca sobrescribe lo que ya existe."""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from pathlib import Path

from zeroagents.work import plan as P

LEAD_PROMPT_FILE = Path(".zeroagents") / "lead_prompt.md"


@dataclass
class ScaffoldReport:
    written: list[Path] = field(default_factory=list)
    skipped: list[Path] = field(default_factory=list)


def _yaml_str(text: str) -> str:
    return json.dumps(" ".join(text.split()), ensure_ascii=False)   # JSON es YAML válido


def agent_file(agent: dict, color: str) -> str:
    tools = ", ".join(agent.get("tools") or []) or "Read, Glob, Grep"
    lines = [
        "---",
        f"name: {agent['name']}",
        f"description: {_yaml_str(agent['description'])}",
        f"model: {agent['model']}",
        f"effort: {agent['effort']}",
        f"tools: {tools}",
        f"color: {color}",
        "---",
        "",
        agent["system_prompt"].strip(),
        "",
        "## Tu contrato en este equipo",
        "",
        f"- Rol: {agent['title']} · fase: {P.PHASE_LABELS[agent['phase']]}. Responsabilidad: {agent['description']}",
        f"- Respondes ante el líder (`team-lead`), que es el {P.LEAD_ROLE}: él prioriza y acepta tu trabajo.",
        "- Lees: " + (", ".join(f"`{x}`" for x in agent.get("inputs") or []) or "lo que te pase el líder") + ".",
        "- Eres dueño de: " + (", ".join(f"`{x}`" for x in agent.get("outputs") or []) or "—") + ".",
        "- Reglas comunes del equipo: `docs/EQUIPO.md`. Plan completo: `docs/PLAN.md`.",
        "- No modifiques archivos de los que no eres dueño: si necesitas un cambio, pídeselo a su dueño o al líder.",
        "- Si te falta información, pregunta al líder o anota la suposición en `docs/decisiones.md`. No inventes.",
        "- Nada de credenciales reales en el código ni en los documentos.",
        "- Al acabar una tarea, márcala como completada y avisa al líder con un resumen de 2-3 líneas.",
        "",
    ]
    return "\n".join(lines)


def plan_md(data: dict) -> str:
    p, lead = data["project"], data["lead"]
    out = [f"# {p['name']}", "", p["summary"], "", f"**Objetivo:** {p['goal']}", ""]

    def section(title: str, items: list[str]) -> None:
        if items:
            out.extend([f"## {title}", "", *[f"- {i}" for i in items], ""])

    section("Usuarios", p["users"])
    section("Tecnologías", p["stack"])
    section("Dentro del alcance", p["in_scope"])
    section("Fuera del alcance", p["out_of_scope"])
    section("Suposiciones", p["assumptions"])
    section("Dudas para el humano", p["open_questions"])

    out += ["## Equipo", "", "| Agente | Rol | Fase | Modelo | Esfuerzo | Por qué |", "|---|---|---|---|---|---|",
            f"| team-lead | {P.LEAD_ROLE}: {P.LEAD_ROLE_DESC} | — | {lead['model']} | {lead['effort']} | {lead['why']} |"]
    for a in data["agents"]:
        out.append(f"| {a['name']} | {a['title']} | {P.PHASE_LABELS[a['phase']]} | {a['model']} "
                   f"| {a['effort']} | {a['why']} |")
    out.append("")

    out += ["## Tareas", "", "| Id | Tarea | Responsable | Fase | Depende de | Terminada cuando |",
            "|---|---|---|---|---|---|"]
    for t in P.task_order(data["tasks"]):
        deps = ", ".join(t.get("blocked_by") or []) or "—"
        out.append(f"| {t['id']} | {t['subject']} | {t['owner']} | {P.PHASE_LABELS[t['phase']]} "
                   f"| {deps} | {t['done_when']} |")
    out.append("")

    if data.get("gates"):
        out += ["## Puntos de control", ""]
        for g in data["gates"]:
            who = " **Aprueba el humano.**" if g["human_approval"] else ""
            out.append(f"- Tras {P.PHASE_LABELS[g['after_phase']]}: {g['condition']}.{who}")
        out.append("")
    return "\n".join(out)


def team_rules_md(data: dict) -> str:
    owners = [(o, a["name"]) for a in data["agents"] for o in a.get("outputs") or []]
    out = [
        "# Reglas del equipo",
        "",
        "Este proyecto lo construye un equipo de agentes de Claude Code coordinado por un líder.",
        "Todos los agentes leen este archivo.",
        "",
        "## Ciclo de vida",
        "",
        "1. Requisitos → 2. Arquitectura (infraestructura y software) → 3. Implementación (código y",
        "refactorización) → 4. Pruebas QA (unitarias, E2E, UAT) y documentación.",
        "No se pasa de fase sin cumplir su punto de control (ver `docs/PLAN.md`).",
        "",
        "## Dueños de los artefactos",
        "",
        "| Artefacto | Dueño |",
        "|---|---|",
        *[f"| `{o}` | {n} |" for o, n in owners],
        "",
        "## Normas",
        "",
        "- Cada agente escribe solo en lo que es suyo; para lo demás, pide el cambio a su dueño.",
        f"- El líder (`team-lead`) es el **{P.LEAD_ROLE}**: {P.LEAD_ROLE_DESC}. El humano es el cliente y",
        "  aprueba los puntos de control.",
        "- Las dudas se preguntan al líder; las suposiciones y decisiones se anotan en `docs/decisiones.md`",
        "  con qué se decidió, por qué y qué alternativa se descartó.",
        "- Quien programa no da por buena su propia tarea: la valida QA.",
        "- Nada de credenciales reales, servicios de pago ni acciones destructivas sin aprobación del humano.",
        "",
    ]
    return "\n".join(out)


def lead_prompt(data: dict) -> str:
    p = data["project"]
    roles = "\n".join(
        f"- `{a['name']}` · **{a['title']}** ({P.PHASE_LABELS[a['phase']]}). {a['description']} "
        f"Dueño de: {', '.join(a.get('outputs') or []) or '—'}. Créalo con el tipo de agente `{a['name']}`, "
        f"que ya fija su modelo ({a['model']}) y su esfuerzo ({a['effort']})."
        for a in data["agents"]
    )
    tasks = "\n".join(
        f"- {t['id']} · {t['subject']} → {t['owner']}"
        + (f" (depende de {', '.join(t['blocked_by'])})" if t.get("blocked_by") else "")
        + f". Aceptada cuando: {t['done_when']}"
        for t in P.task_order(data["tasks"])
    )
    human = [g for g in data.get("gates") or [] if g["human_approval"]]
    gates = "\n".join(f"- Tras {P.PHASE_LABELS[g['after_phase']]}: {g['condition']}" for g in human) \
        or "- Tras requisitos y tras arquitectura."
    return f"""\
Eres el **{P.LEAD_ROLE}** (team-lead) del equipo que va a construir **{p['name']}**.

{p['summary']}

Tu rol: {P.LEAD_ROLE_DESC}. No programas: delegas en el equipo. Yo soy el cliente (stakeholder):
apruebo los puntos de control y resuelvo las dudas de producto.

Lee primero `docs/PLAN.md` (planteamiento, equipo, tareas y puntos de control) y `docs/EQUIPO.md`
(reglas comunes). Las definiciones de cada agente están en `.claude/agents/`.

## Qué tienes que hacer

1. Crea un equipo de agentes con estos compañeros. Al crear cada uno, **dile explícitamente su rol,
   sus responsabilidades y de qué artefactos es dueño**, y créalo a partir de su tipo de agente para
   que conserve su modelo, esfuerzo, herramientas y system prompt:
{roles}
2. Crea el backlog en la lista de tareas del equipo, con responsables, dependencias y criterio de
   aceptación:
{tasks}
3. Como Product Owner: asigna solo tareas desbloqueadas, en orden de valor. Cuando alguien entregue,
   **comprueba el criterio de aceptación** y acepta la tarea o devuélvela con lo que falta. No dejes
   pasar de fase sin cumplir su punto de control.
4. **Para y pide mi aprobación** antes de seguir en estos puntos:
{gates}
5. Si un compañero tiene una duda de producto que el plan no resuelve, pregúntamela a mí.

Empieza presentándote como Product Owner, resúmeme el plan y los roles en pocas líneas, dime las
dudas abiertas, crea el equipo y arranca la primera fase.
"""


def write(directory: Path, data: dict, meta: dict) -> ScaffoldReport:
    report = ScaffoldReport()

    def put(rel: Path | str, content: str) -> None:
        path = directory / rel
        if path.exists():
            report.skipped.append(path)
            return
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        report.written.append(path)

    for i, agent in enumerate(data["agents"]):
        put(Path(".claude") / "agents" / f"{agent['name']}.md", agent_file(agent, P.COLORS[i % len(P.COLORS)]))
    put("docs/PLAN.md", plan_md(data))
    put("docs/EQUIPO.md", team_rules_md(data))
    put("docs/decisiones.md", "# Registro de decisiones\n\nFormato: fecha · decisión · por qué · "
                              "alternativa descartada · quién.\n\n")
    put(LEAD_PROMPT_FILE, lead_prompt(data))
    # team.json es la fuente del panel. Si ya había uno, se guarda una copia antes.
    old = directory / P.PLAN_FILE
    if old.exists():
        backup = old.with_name(f"team.{time.strftime('%Y%m%d-%H%M%S')}.json")
        old.replace(backup)
        report.skipped.append(backup)
    P.save(directory, {"zeroagents": 1, "created": time.time(), **meta, "plan": data})
    report.written.append(directory / P.PLAN_FILE)
    return report
