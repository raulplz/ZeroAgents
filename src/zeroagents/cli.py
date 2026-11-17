"""Punto de entrada. Los subcomandos `hook` y `statusline` los llama Claude Code
a menudo, así que solo importan lo imprescindible (nada de Textual)."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def _read_stdin() -> tuple[str, dict]:
    raw = sys.stdin.read()
    try:
        return raw, json.loads(raw) if raw.strip() else {}
    except ValueError:
        return raw, {}


def cmd_hook(args) -> int:
    _, data = _read_stdin()
    if args.platform == "claude-code":
        from zeroagents.adapters.claude_code import ingest_hook

        try:
            ingest_hook(data, notify=not args.no_notify)
        except Exception as exc:  # un hook nunca debe molestar a Claude
            print(f"zeroagents: {exc}", file=sys.stderr)
    return 0


def cmd_statusline(args) -> int:
    raw, data = _read_stdin()
    from zeroagents.adapters.claude_code import ingest_statusline

    try:
        ingest_statusline(data)
    except Exception as exc:
        print(f"zeroagents: {exc}", file=sys.stderr)
    if args.chain:
        try:
            out = subprocess.run(args.chain, shell=True, input=raw, capture_output=True,
                                 text=True, timeout=5)
            sys.stdout.write(out.stdout)
        except subprocess.SubprocessError:
            pass
    else:
        model = (data.get("model") or {}).get("display_name") or "?"
        where = (data.get("workspace") or {}).get("current_dir") or data.get("cwd") or ""
        from zeroagents.fmt import place

        sys.stdout.write(f"\033[36m{model}\033[0m · \033[34m{place(where)}\033[0m")
    return 0


def cmd_list(args) -> int:
    from zeroagents import fmt
    from zeroagents.monitor import Monitor

    snap = Monitor().snapshot(include_ended=args.all)
    if args.json:
        payload = {
            "agents": [
                {
                    "platform": a.platform, "id": a.id, "name": a.name, "cwd": a.cwd,
                    "status": a.status.value, "needs_you": a.status.needs_you,
                    "attention": a.attention_msg, "model": a.model, "effort": a.effort,
                    "mode": a.mode, "fast": a.fast, "pid": a.pid, "alive": a.alive,
                    "subagents": a.subagents, "last_activity": a.last_activity,
                    "usage": vars(a.usage),
                }
                for a in snap.agents
            ],
            "limits": {k: vars(v) for k, v in snap.limits.items()},
        }
        print(json.dumps(payload, ensure_ascii=False, indent=2))
        return 0
    if not snap.agents:
        print("No hay agentes abiertos.")
        return 0
    rows = [("ESTADO", "AGENTE", "MODELO", "ESFUERZO", "MODO", "CONTEXTO", "TOKENS", "COSTE", "HACE")]
    for a in snap.agents:
        rows.append((
            f"{fmt.ICONS[a.status]} {a.status.value}", fmt.clip(a.name, 28), fmt.model(a.model),
            fmt.effort(a.effort), fmt.mode(a.mode),
            "—" if a.usage.context_pct is None else f"{a.usage.context_pct:.0f}%",
            fmt.tokens(a.usage.input_tokens + a.usage.output_tokens),
            fmt.cost(a.usage.cost_usd), fmt.ago(a.last_activity),
        ))
    widths = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    for r in rows:
        print("  ".join(c.ljust(w) for c, w in zip(r, widths)).rstrip())
    for a in snap.agents:
        if a.status.needs_you:
            print(f"\n⚠ {a.name}: {a.attention_msg or 'te necesita'}")
    return 0


def cmd_messages(args) -> int:
    import time
    from pathlib import Path

    from zeroagents import messages as M

    project = None
    if args.dir:
        project = Path(args.dir).expanduser().resolve()
    elif not args.all_sessions and (Path.cwd() / M.PROJECT_LOG).exists():
        project = Path.cwd()

    def paths():
        return M.sources(project, args.session)

    color = M.use_color() and not args.md and not args.json
    if args.follow:
        where = f"{project / M.PROJECT_LOG}" if project else "todas las sesiones"
        print(f"Mensajes entre agentes en vivo ({where}). Ctrl+C para salir.", flush=True)
        M.follow(paths, full=args.full, color=color)
        return 0
    since = None if project or args.hours <= 0 else time.time() - args.hours * 3600
    msgs = M.load(paths(), since)
    if args.json:
        for m in msgs:
            print(json.dumps(m, ensure_ascii=False))
    elif args.md:
        print(M.to_markdown(msgs))
    elif not msgs:
        print("No hay mensajes entre agentes todavía.")
    else:
        print("\n\n".join(M.format_message(m, full=args.full, color=color) for m in msgs))
    return 0


def cmd_install(args) -> int:
    from zeroagents.install import install

    for note in install(notify=not args.no_notify, statusline=not args.no_statusline):
        print("·", note)
    print("Las sesiones de Claude Code ya abiertas cogen los hooks al recargar la configuración;"
          " si no aparecen, reinícialas.")
    return 0


def cmd_uninstall(args) -> int:
    from zeroagents.install import uninstall

    for note in uninstall():
        print("·", note)
    return 0


def cmd_doctor(args) -> int:
    from zeroagents.adapters.claude_code import read_registry
    from zeroagents.install import executable, status
    from zeroagents.paths import platform_dir

    st = status()
    print(f"Ejecutable: {executable()}")
    print(f"Hooks instalados: {len(st['hooks'])}/{len(st['hooks']) + len(st['missing_hooks'])}"
          + (f" (faltan: {', '.join(st['missing_hooks'])})" if st["missing_hooks"] else ""))
    print(f"Statusline envuelta: {'sí' if st['statusline'] else 'no'}")
    directory = platform_dir("claude-code")
    print(f"Estado en: {directory} ({len(list(directory.glob('*')))} archivos)")
    print(f"Sesiones en el registro de Claude Code: {len(read_registry())}")
    return 0


def cmd_tui(args) -> int:
    from zeroagents.tui import run

    run(work=getattr(args, "work", False))
    return 0


# ── modo Work ───────────────────────────────────────────────────────────────

def _project_dir(value: str | None, name_hint: str | None = None) -> "Path":
    from pathlib import Path

    from zeroagents.work.plan import slugify

    if value:
        return Path(value).expanduser().resolve()
    return (Path.cwd() / slugify(name_hint or "proyecto")).resolve()


def create_project(prompt: str, directory, models, model: str, effort: str, log=print,
                   launch_mode: str | None = None):
    """Ejecuta el arquitecto y escribe el proyecto. Lo usan la CLI y el panel."""
    from zeroagents.work import architect, live, scaffold

    log(f"Arquitecto de equipos ({model}, esfuerzo {effort}) pensando el equipo para {directory} …")
    result = architect.run(prompt, directory, models=models, model=model, effort=effort)
    meta = {"prompt": prompt, "architect": {"model": model, "effort": effort,
                                            "cost_usd": result.cost_usd, "duration_ms": result.duration_ms,
                                            "session_id": result.session_id}}
    if launch_mode:
        meta["launch"] = {"permission_mode": launch_mode}
    report = scaffold.write(directory, result.plan, meta)
    live.register(directory, result.plan["project"]["name"])
    return result, report


def _print_plan(data: dict) -> None:
    from zeroagents import fmt
    from zeroagents.work.plan import PHASE_LABELS, task_order

    plan = data["plan"]
    p, lead = plan["project"], plan["lead"]
    print(f"\n❄ {p['name']}\n  {p['summary']}\n")
    rows = [("AGENTE", "ROL", "FASE", "MODELO", "ESFUERZO"),
            ("team-lead", "Product Owner", "—", lead["model"], fmt.effort(lead["effort"]))]
    rows += [(a["name"], fmt.clip(a["title"], 30), PHASE_LABELS[a["phase"]], a["model"], fmt.effort(a["effort"]))
             for a in plan["agents"]]
    widths = [max(len(r[i]) for r in rows) for i in range(5)]
    for r in rows:
        print("  " + "  ".join(c.ljust(w) for c, w in zip(r, widths)).rstrip())
    print(f"\n  {len(plan['tasks'])} tareas: " + " → ".join(t["id"] for t in task_order(plan["tasks"])[:12])
          + (" …" if len(plan["tasks"]) > 12 else ""))
    if p["open_questions"]:
        print("\n  Dudas para ti:")
        for q in p["open_questions"]:
            print(f"   · {q}")


def cmd_work_new(args) -> int:
    from zeroagents.work import architect
    from zeroagents.work.plan import ALL_MODELS, DEFAULT_MODELS

    prompt = args.prompt
    if prompt == "-" or not prompt:
        prompt = sys.stdin.read()
    if args.file:
        prompt = open(args.file, encoding="utf-8").read()
    if not prompt.strip():
        print("Falta el prompt del proyecto.", file=sys.stderr)
        return 2
    models = tuple(m.strip() for m in args.models.split(",")) if args.models else DEFAULT_MODELS
    if bad := [m for m in models if m not in ALL_MODELS]:
        print(f"Modelos no válidos: {', '.join(bad)} (usa {', '.join(ALL_MODELS)})", file=sys.stderr)
        return 2
    directory = _project_dir(args.dir, args.name or prompt.split("\n")[0][:40])
    try:
        result, report = create_project(prompt, directory, models, args.architect_model, args.architect_effort,
                                        launch_mode=args.permission_mode)
    except architect.ArchitectError as exc:
        print(f"✗ {exc}", file=sys.stderr)
        return 1
    from zeroagents.work.plan import load

    _print_plan(load(directory))
    print(f"\n  Archivos en {directory}:")
    for path in report.written:
        print(f"   + {path.relative_to(directory)}")
    for path in report.skipped:
        print(f"   = {path.relative_to(directory)} (ya existía, no se toca)")
    cost = f"≈${result.cost_usd:.2f}" if result.cost_usd is not None else "?"
    print(f"\n  Coste del arquitecto: {cost} (precio de lista)."
          f"\n  Revisa docs/PLAN.md y, cuando quieras, arranca el equipo con:"
          f"\n    zeroagents work start {directory}")
    return 0


def cmd_work_list(args) -> int:
    from zeroagents.monitor import Monitor
    from zeroagents.work import live

    dirs = live.projects()
    if not dirs:
        print("No hay proyectos del modo Work. Crea uno con: zeroagents work new \"…\"")
        return 0
    sessions = Monitor().snapshot(include_ended=True).agents
    for d in dirs:
        view = live.build(d, sessions)
        if not view:
            continue
        done, total = view.progress
        lead = view.lead.status.value if view.lead and view.lead.alive else "sin líder abierto"
        print(f"❄ {view.name:<28} {done}/{total} tareas · {view.phase:<20} · líder: {lead}\n   {d}")
    return 0


def cmd_work_show(args) -> int:
    from zeroagents.monitor import Monitor
    from zeroagents.work import live

    directory = _project_dir(args.dir or ".")
    view = live.build(directory, Monitor().snapshot(include_ended=True).agents)
    if not view:
        print(f"No hay plan en {directory}.", file=sys.stderr)
        return 1
    _print_plan(view.data)
    done, total = view.progress
    origin = f"equipo {view.team_name}" if view.live_tasks else "plan (el equipo aún no ha creado tareas)"
    print(f"\n  Progreso: {done}/{total} · fase: {view.phase} · tareas de: {origin}")
    if view.lead:
        need = f" — {view.lead.attention_msg}" if view.lead.status.needs_you else ""
        print(f"  Líder: {view.lead.status.value}{need}")
    print()
    for m in view.members:
        cur = f": {m.current}" if m.current else ""
        print(f"  {m.name:<22} {m.state}{cur}")
    print()
    for t in view.tasks:
        mark = {"hecha": "✓", "en curso": "◐"}.get(t.status, "·" if not t.blocked else "⧗")
        print(f"  {mark} {t.id:<5} {fmt_clip(t.subject, 60):<60} {t.owner or ''}")
    return 0


def fmt_clip(text: str, width: int) -> str:
    from zeroagents.fmt import clip

    return clip(text, width)


def cmd_work_start(args) -> int:
    from zeroagents.work import launch

    directory = _project_dir(args.dir or ".")
    try:
        used = launch.launch(directory, permission_mode=args.permission_mode)
    except RuntimeError as exc:
        print(f"✗ {exc}", file=sys.stderr)
        return 1
    print(f"Líder abierto en una terminal nueva:\n  {used}\nSíguelo con: zeroagents --work")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="zeroagents", description="Monitor Frost de agentes de IA.")
    sub = parser.add_subparsers(dest="cmd")

    parser.add_argument("--work", action="store_true", help="abre el panel directamente en el modo Work")
    sub.add_parser("tui", help="abre el panel (por defecto)").set_defaults(func=cmd_tui)

    work = sub.add_parser("work", help="modo Work: equipos de agentes por proyecto")
    wsub = work.add_subparsers(dest="work_cmd", required=True)
    p = wsub.add_parser("new", help="un arquitecto de equipos diseña el equipo y el proyecto a partir de tu prompt")
    p.add_argument("prompt", nargs="?", help="descripción del proyecto ('-' para leerla de la entrada)")
    p.add_argument("-f", "--file", help="leer el prompt de un archivo")
    p.add_argument("-d", "--dir", help="carpeta del proyecto (por defecto, una nueva aquí con el nombre)")
    p.add_argument("-n", "--name", help="nombre para la carpeta si no das --dir")
    p.add_argument("--models", help="modelos que puede usar el equipo, p. ej. haiku,sonnet,opus,fable")
    p.add_argument("--architect-model", default="opus", help="modelo del arquitecto (opus)")
    p.add_argument("--architect-effort", default="high", help="esfuerzo del arquitecto (high)")
    p.add_argument("--permission-mode", choices=["default", "acceptEdits", "plan", "auto"],
                   help="modo de permisos con el que arrancará el líder (se guarda en el proyecto)")
    p.set_defaults(func=cmd_work_new)
    wsub.add_parser("list", help="proyectos del modo Work").set_defaults(func=cmd_work_list)
    p = wsub.add_parser("show", help="equipo, tareas y progreso de un proyecto")
    p.add_argument("dir", nargs="?")
    p.set_defaults(func=cmd_work_show)
    p = wsub.add_parser("start", help="abre el líder del equipo en una terminal nueva")
    p.add_argument("dir", nargs="?")
    p.add_argument("--permission-mode", choices=["default", "acceptEdits", "plan", "auto"],
                   help="modo de permisos del líder (por defecto, el tuyo)")
    p.set_defaults(func=cmd_work_start)

    p = sub.add_parser("list", help="lista los agentes en texto")
    p.add_argument("--json", action="store_true")
    p.add_argument("--all", action="store_true", help="incluye sesiones cerradas en las últimas 24 h")
    p.set_defaults(func=cmd_list)

    p = sub.add_parser("messages", aliases=["mensajes"],
                       help="encargos, entregas y mensajes entre agentes (-f para verlos en vivo)")
    p.add_argument("dir", nargs="?", help="proyecto con .zeroagents/ (por defecto, la carpeta actual si lo tiene)")
    p.add_argument("-f", "--follow", action="store_true", help="mostrar los nuevos según llegan")
    p.add_argument("--full", action="store_true", help="textos completos")
    p.add_argument("--md", action="store_true", help="en Markdown, para el registro de un proyecto")
    p.add_argument("--json", action="store_true", help="una línea JSON por mensaje")
    p.add_argument("--session", help="solo esta sesión (id o prefijo)")
    p.add_argument("--all-sessions", action="store_true", help="ignorar el registro del proyecto actual")
    p.add_argument("--hours", type=float, default=24, help="sin proyecto, últimas N horas (0 = todo)")
    p.set_defaults(func=cmd_messages)

    p = sub.add_parser("hook", help="(lo usa Claude Code) registra un evento de hook")
    p.add_argument("platform", choices=["claude-code"])
    p.add_argument("--no-notify", action="store_true")
    p.set_defaults(func=cmd_hook)

    p = sub.add_parser("statusline", help="(lo usa Claude Code) guarda la statusline y la pinta")
    p.add_argument("--chain", help="statusline original a la que pasar los datos")
    p.set_defaults(func=cmd_statusline)

    p = sub.add_parser("install", help="conecta ZeroAgents con Claude Code")
    p.add_argument("--no-notify", action="store_true", help="sin notificaciones de escritorio")
    p.add_argument("--no-statusline", action="store_true", help="no tocar la statusline")
    p.set_defaults(func=cmd_install)

    sub.add_parser("uninstall", help="quita hooks y restaura la statusline").set_defaults(func=cmd_uninstall)
    sub.add_parser("doctor", help="comprueba la instalación").set_defaults(func=cmd_doctor)

    args = parser.parse_args(argv)
    return (getattr(args, "func", None) or cmd_tui)(args)
