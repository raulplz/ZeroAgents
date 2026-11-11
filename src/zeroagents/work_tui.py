"""Pantalla Work del panel: proyectos, su equipo de agentes y sus tareas."""

from __future__ import annotations

from pathlib import Path

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen, Screen
from textual.widgets import Button, DataTable, Footer, Input, Label, Markdown, Select, Static, TextArea

from zeroagents import fmt
from zeroagents.focus import focus_pid
from zeroagents.model import Status
from zeroagents.work import live
from zeroagents.work.plan import ALL_MODELS, DEFAULT_MODELS, LEAD_ROLE, slugify

STATE_STYLE = {
    "trabajando": "cyan", "con tarea lista": "yellow", "esperando dependencias": "dim",
    "terminado": "green", "libre": "dim", "no creado": "dim", "sin arrancar": "dim",
}
TASK_ICON = {live.DONE: ("✓", "green"), live.RUNNING: ("◐", "cyan")}
ARCHITECTS = [("Rápido · Sonnet, esfuerzo medio (~30 s)", "sonnet:medium"),
              ("Calidad · Opus, esfuerzo alto (1-3 min, más caro)", "opus:high")]
PERMISSIONS = [("Aceptar ediciones de archivos (Bash sigue preguntando)", "acceptEdits"),
               ("Preguntar todo", "default")]


class NewProject(ModalScreen[dict | None]):
    """Formulario: prompt del proyecto, carpeta y modelos permitidos."""

    BINDINGS = [Binding("escape", "cancel", "Cancelar")]

    def __init__(self, colors: dict) -> None:
        super().__init__()
        self.c = colors

    def compose(self) -> ComposeResult:
        with Vertical(id="form"):
            yield Label("❄ Nuevo proyecto · el arquitecto de equipos diseñará el equipo y el plan", id="form-title")
            yield Label("Describe el proyecto: qué es, para quién, tecnologías o límites que quieras.")
            yield TextArea(id="prompt", soft_wrap=True)
            yield Label("Carpeta (vacío = ~/Work/PROYECTOS/<nombre>)")
            yield Input(placeholder="~/Work/PROYECTOS/mi-proyecto", id="dir")
            yield Label(f"Modelos que puede usar el equipo ({', '.join(ALL_MODELS)})")
            yield Input(value=",".join(DEFAULT_MODELS), id="models")
            yield Label("Arquitecto de equipos")
            yield Select(ARCHITECTS, value="sonnet:medium", allow_blank=False, id="architect")
            yield Label("Permisos del equipo al arrancar")
            yield Select(PERMISSIONS, value="acceptEdits", allow_blank=False, id="perms")
            with Horizontal(id="buttons"):
                yield Button("Diseñar equipo", variant="primary", id="ok")
                yield Button("Cancelar", id="cancel")

    def on_mount(self) -> None:
        self.query_one("#prompt", TextArea).focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "cancel":
            self.dismiss(None)
            return
        prompt = self.query_one("#prompt", TextArea).text.strip()
        if not prompt:
            self.notify("Escribe primero el prompt del proyecto.", severity="warning")
            return
        models = tuple(m.strip() for m in self.query_one("#models", Input).value.split(",") if m.strip())
        bad = [m for m in models if m not in ALL_MODELS]
        if bad or not models:
            self.notify(f"Modelos no válidos: {', '.join(bad) or 'ninguno'}", severity="warning")
            return
        raw_dir = self.query_one("#dir", Input).value.strip()
        directory = Path(raw_dir).expanduser() if raw_dir else \
            Path.home() / "Work" / "PROYECTOS" / slugify(prompt.split("\n")[0][:40])
        arch_model, arch_effort = str(self.query_one("#architect", Select).value).split(":")
        self.dismiss({"prompt": prompt, "dir": directory.resolve(), "models": models,
                      "arch_model": arch_model, "arch_effort": arch_effort,
                      "perms": str(self.query_one("#perms", Select).value)})

    def action_cancel(self) -> None:
        self.dismiss(None)


class PlanView(ModalScreen):
    """docs/PLAN.md dentro de la app."""

    BINDINGS = [Binding("escape,p,q", "close", "Cerrar")]

    def __init__(self, text: str, title: str) -> None:
        super().__init__()
        self.text, self.title_text = text, title

    def compose(self) -> ComposeResult:
        with Vertical(id="planbox"):
            yield Markdown(self.text, id="plan")
        yield Footer()

    def on_mount(self) -> None:
        box = self.query_one("#planbox")
        box.border_title = f"plan · {self.title_text}"
        box.focus()

    def action_close(self) -> None:
        self.dismiss()


class WorkScreen(Screen):
    BINDINGS = [
        Binding("w,escape", "back", "Volver a sesiones"),
        Binding("n", "new", "Nuevo proyecto"),
        Binding("l", "launch", "Arrancar equipo"),
        Binding("f", "focus_lead", "Ir al Product Owner"),
        Binding("p", "show_plan", "Ver plan"),
        Binding("o", "open_code", "Abrir en VS Code"),
        Binding("tab", "switch", "Cambiar tabla"),
        Binding("r", "refresh_data", "Refrescar"),
        Binding("a", "noop", show=False),   # «mostrar cerradas» es de la vista de sesiones
        Binding("q", "app.quit", "Salir"),
    ]

    def action_noop(self) -> None:
        pass

    def action_refresh_data(self) -> None:
        self.refresh_data()

    def __init__(self, colors: dict, monitor) -> None:
        super().__init__()
        self.c = colors
        self.monitor = monitor
        self.views: list[live.ProjectView] = []
        self.busy: str | None = None
        self.tick = 0

    def compose(self) -> ComposeResult:
        with Horizontal(id="wtop"):
            yield Static(Text("❄ ZERØ WORK"), id="wbrand")
            yield Static(id="wsummary")
        yield Static(id="walert")
        yield Static("No hay proyectos. Pulsa n para crear uno a partir de un prompt.", id="wempty")
        with Horizontal(id="wbody"):
            yield DataTable(id="projects", cursor_type="row")
            with Vertical(id="wright"):
                yield DataTable(id="team", cursor_type="row")
                yield DataTable(id="tasks", cursor_type="row")
        yield Static(id="wdetail")
        yield Footer()

    def on_mount(self) -> None:
        self.query_one("#projects", DataTable).add_columns("Proyecto", "Progreso", "Fase")
        self.query_one("#team", DataTable).add_columns("Agente", "Rol", "Modelo", "Esfuerzo", "Estado", "Ahora")
        self.query_one("#tasks", DataTable).add_columns("", "Id", "Tarea", "Responsable", "Estado")
        self.query_one("#team").border_title = "equipo"
        self.query_one("#tasks").border_title = "tareas"
        self.query_one("#projects").border_title = "proyectos"
        self.refresh_data()
        self.set_interval(1.5, self.refresh_data)

    # ── datos ──────────────────────────────────────────────────────────────

    def _selected(self) -> live.ProjectView | None:
        table = self.query_one("#projects", DataTable)
        if not self.views or table.cursor_row >= len(self.views):
            return None
        return self.views[table.cursor_row]

    def refresh_data(self) -> None:
        self.tick += 1
        sessions = self.monitor.snapshot(include_ended=True).agents
        self.views = [v for d in live.projects() if (v := live.build(d, sessions))]
        table = self.query_one("#projects", DataTable)
        row = table.cursor_row
        table.clear()
        for v in self.views:
            done, total = v.progress
            table.add_row(Text(fmt.clip(v.name, 26), style=f"bold {self.c['fg']}"),
                          Text(fmt.bar(100 * done / total if total else None), style=self.c["cyan"]),
                          Text(v.phase, style=self.c["dim"]))
        if self.views:
            table.move_cursor(row=min(row, len(self.views) - 1), animate=False)
        empty = not self.views
        self.query_one("#wempty").display = empty
        self.query_one("#wbody").display = not empty
        self._render_summary()
        self._render_project()

    def _render_summary(self) -> None:
        t = Text(f"{len(self.views)} proyectos", style=self.c["fg"])
        running = sum(1 for v in self.views if v.lead and v.lead.alive)
        t.append(f"  ·  {running} con líder abierto", style=self.c["cyan"])
        if self.busy:
            t.append(f"  ·  {'◐◓◑◒'[self.tick % 4]} {self.busy}", style=f"bold {self.c['yellow']}")
        self.query_one("#wsummary", Static).update(t)

    def _render_project(self) -> None:
        v = self._selected()
        team = self.query_one("#team", DataTable)
        tasks = self.query_one("#tasks", DataTable)
        team.clear()
        tasks.clear()
        alert = self.query_one("#walert", Static)
        detail = self.query_one("#wdetail", Static)
        if not v:
            alert.display = False
            detail.update("")
            return
        lead = v.data["plan"]["lead"]
        lead_state = v.lead.status.value if v.lead and v.lead.alive else "sin abrir"
        lead_style = self.c["red"] if v.lead and v.lead.status.needs_you else self.c["cyan"]
        team.add_row(Text("team-lead", style=f"bold {self.c['ice']}"), Text(LEAD_ROLE, style=self.c["lavender"]),
                     Text(lead["model"], style=self.c["accent"]), Text(fmt.effort(lead["effort"])),
                     Text(lead_state, style=lead_style), Text(""))
        for m in v.members:
            style = self.c.get(STATE_STYLE.get(m.state, "dim"), self.c["dim"])
            team.add_row(Text(m.name, style=f"bold {self.c['fg']}" if m.in_team else self.c["fg"]),
                         Text(fmt.clip(m.title, 24), style=self.c["dim"]),
                         Text(m.model, style=self.c["accent"]), Text(fmt.effort(m.effort)),
                         Text(m.state, style=style), Text(fmt.clip(m.current or "", 40), style=self.c["fg"]))
        for t in v.tasks:
            icon, color = TASK_ICON.get(t.status, ("⧗", "muted") if t.blocked else ("·", "dim"))
            state = "bloqueada" if t.blocked else t.status
            tasks.add_row(Text(icon, style=self.c[color]), Text(t.id, style=self.c["dim"]),
                          Text(fmt.clip(t.subject, 60), style=self.c["fg"]),
                          Text(t.owner or "—", style=self.c["lavender"]), Text(state, style=self.c[color]))

        tasks.border_title = "tareas" + ("" if v.live_tasks else " (del plan; el equipo aún no las ha creado)")
        needs = v.lead and v.lead.alive and v.lead.status.needs_you
        alert.display = bool(needs)
        if needs:
            alert.update(Text(f"⚠ El líder te necesita: {v.lead.attention_msg or v.lead.status.value}  (f para ir)",
                              style=f"bold {self.c['red']}"))
        done, total = v.progress
        d = Text()
        d.append(f"{v.name}", style=f"bold {self.c['ice']}")
        d.append(f"  {fmt.place(str(v.directory))}\n", style=self.c["dim"])
        d.append(v.data["plan"]["project"]["summary"] + "\n", style=self.c["fg"])
        d.append(f"Progreso {done}/{total} · fase {v.phase}", style=self.c["cyan"])
        d.append(f" · equipo {v.team_name or 'sin crear'}", style=self.c["dim"])
        if qs := v.data["plan"]["project"]["open_questions"]:
            d.append(f"\nDudas abiertas: {' · '.join(qs[:3])}", style=self.c["yellow"])
        detail.update(d)

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.data_table.id == "projects":
            self._render_project()

    # ── acciones ───────────────────────────────────────────────────────────

    def action_back(self) -> None:
        self.app.pop_screen()

    def action_switch(self) -> None:
        self.focus_next(DataTable)

    def action_focus_lead(self) -> None:
        v = self._selected()
        error = focus_pid(v.lead.pid if v and v.lead and v.lead.alive else None)
        if error:
            self.notify(error if v and v.lead else "Este proyecto no tiene el líder abierto (pulsa l).",
                        severity="warning")

    def action_show_plan(self) -> None:
        from zeroagents.work.scaffold import plan_md

        v = self._selected()
        if not v:
            return
        path = v.directory / "docs" / "PLAN.md"
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            text = plan_md(v.data["plan"])
        self.app.push_screen(PlanView(text, v.name))

    def action_open_code(self) -> None:
        import shutil
        import subprocess

        v = self._selected()
        if not v:
            return
        opener = ["code", str(v.directory)] if shutil.which("code") else ["xdg-open", str(v.directory)]
        try:
            subprocess.Popen(opener, start_new_session=True,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.notify(f"Abriendo {fmt.place(str(v.directory))}")
        except OSError as exc:
            self.notify(f"No he podido abrir la carpeta: {exc}", severity="error")

    def action_launch(self) -> None:
        from zeroagents.work import launch

        v = self._selected()
        if not v:
            return
        if v.lead and v.lead.alive and v.lead.status is not Status.ENDED:
            self.notify("El líder ya está abierto: pulsa f para ir a su ventana.")
            return
        try:
            launch.launch(v.directory)
            self.notify(f"Líder de «{v.name}» abierto en una terminal nueva.")
        except RuntimeError as exc:
            self.notify(str(exc), severity="error")

    def action_new(self) -> None:
        if self.busy:
            self.notify("Ya hay un arquitecto trabajando; espera a que termine.", severity="warning")
            return
        self.app.push_screen(NewProject(self.c), self._create)

    def _create(self, values: dict | None) -> None:
        if values:
            self.busy = f"arquitecto diseñando {values['dir'].name}"
            self._create_worker(values)

    @work(thread=True, exclusive=True)
    def _create_worker(self, values: dict) -> None:
        from zeroagents.cli import create_project
        from zeroagents.work.architect import ArchitectError

        try:
            result, report = create_project(values["prompt"], values["dir"], values["models"],
                                            values["arch_model"], values["arch_effort"],
                                            log=lambda _m: None, launch_mode=values["perms"])
            cost = f" (≈${result.cost_usd:.2f})" if result.cost_usd is not None else ""
            msg, sev = (f"Equipo listo para «{result.plan['project']['name']}»{cost}. "
                        "Revisa docs/PLAN.md y pulsa l para arrancar.", "information")
        except (ArchitectError, OSError) as exc:
            msg, sev = f"El arquitecto ha fallado: {exc}", "error"
        self.app.call_from_thread(self._created, msg, sev)

    def _created(self, msg: str, severity: str) -> None:
        self.busy = None
        self.notify(msg, severity=severity, timeout=12)
        self.refresh_data()


def css(c: dict) -> str:
    return f"""
    #wtop {{ height: 3; padding: 1 2 0 2; }}
    #wbrand {{ width: auto; color: {c['ice']}; text-style: bold; }}
    #wsummary {{ width: 1fr; padding-left: 3; }}
    #walert {{ height: auto; padding: 0 2; display: none; }}
    #wempty {{ padding: 2 4; color: {c['dim']}; }}
    #wbody {{ height: 1fr; margin: 0 1; }}
    #projects {{ width: 58; margin-right: 1; }}
    #wright {{ width: 1fr; }}
    #team {{ height: auto; max-height: 14; margin-bottom: 1; }}
    #tasks {{ height: 1fr; }}
    WorkScreen DataTable {{ margin: 0; border: round #1c2a3a; border-title-color: {c['accent']};
                            background: {c['bg']}; }}
    WorkScreen DataTable > .datatable--header {{ background: {c['bg']}; color: {c['dim']}; text-style: bold; }}
    WorkScreen DataTable > .datatable--cursor {{ background: #153354; }}
    WorkScreen DataTable:focus {{ border: round {c['accent']}; }}
    #wdetail {{ height: auto; max-height: 8; margin: 0 1; padding: 0 2;
               border: round #1c2a3a; background: #0c121a; }}
    NewProject {{ align: center middle; }}
    #form {{ width: 90; height: auto; max-height: 95%; overflow-y: auto; padding: 1 2; border: round {c['accent']}; background: {c['panel']}; }}
    #form-title {{ color: {c['ice']}; text-style: bold; margin-bottom: 1; }}
    #form Label {{ color: {c['dim']}; margin-top: 1; }}
    #prompt {{ height: 8; }}
    #form Select {{ width: 100%; }}
    PlanView {{ align: center middle; }}
    #planbox {{ width: 90%; height: 90%; padding: 0 1; border: round {c['accent']};
               border-title-color: {c['ice']}; background: {c['panel']}; overflow-y: auto; }}
    #plan {{ background: {c['panel']}; }}
    #buttons {{ height: auto; margin-top: 1; align-horizontal: right; }}
    #buttons Button {{ margin-left: 2; }}
    """
