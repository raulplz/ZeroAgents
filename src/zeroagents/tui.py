"""Panel ZeroAgents (Textual) con la paleta Frost de obsidian-blue."""

from __future__ import annotations

from rich.text import Text
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal
from textual.theme import Theme
from textual.widgets import DataTable, Footer, Static

from zeroagents import fmt
from zeroagents.focus import focus_pid
from zeroagents.model import Agent, Snapshot, Status
from zeroagents.monitor import Monitor
from zeroagents.work_tui import WorkScreen, css as work_css

C = {
    "accent": "#4da3ff", "cyan": "#75c4e8", "ice": "#9cdcf5", "lavender": "#9faee8",
    "fg": "#d5dfeb", "muted": "#536477", "dim": "#8292a6", "red": "#ef7180",
    "yellow": "#e6bf79", "green": "#8fbea0", "bg": "#080b10", "panel": "#111923",
}

FROST = Theme(
    name="frost",
    primary=C["accent"], secondary=C["cyan"], accent=C["lavender"],
    foreground=C["fg"], background=C["bg"], surface="#0c121a", panel=C["panel"],
    success=C["green"], warning=C["yellow"], error=C["red"], dark=True,
)

STATUS_STYLE = {
    Status.PERMISSION: f"bold {C['red']}",
    Status.QUESTION: f"bold {C['yellow']}",
    Status.WORKING: C["cyan"],
    Status.YOUR_TURN: C["green"],
    Status.UNKNOWN: C["dim"],
    Status.ENDED: C["muted"],
}
SPINNER = "◐◓◑◒"
COLUMNS = ("Estado", "Agente", "Plat.", "Carpeta", "Modelo", "Esfuerzo", "Modo",
           "Contexto", "Tokens", "Coste", "Hace")


def _limit_text(label: str, pct: float | None, resets: float | None) -> Text:
    if pct is None:
        return Text("")
    color = C["red"] if pct >= 85 else C["yellow"] if pct >= 60 else C["cyan"]
    out = Text(f"{label} ", style=C["dim"])
    out.append(fmt.bar(pct), style=color)
    if resets:
        out.append(f" ({fmt.clock(resets)})", style=C["muted"])
    return out


class ZeroAgents(App):
    TITLE = "ZeroAgents"
    ENABLE_COMMAND_PALETTE = False
    CSS = f"""
    Screen {{ background: {C['bg']}; }}
    #top {{ height: 3; padding: 1 2 0 2; }}
    #brand {{ width: auto; color: {C['ice']}; text-style: bold; }}
    #summary {{ width: 1fr; padding-left: 3; }}
    #limits {{ width: auto; }}
    #alert {{ height: auto; padding: 0 2; color: {C['red']}; display: none; }}
    #alert.on {{ display: block; }}
    DataTable {{ height: 1fr; margin: 1 1 0 1; background: {C['bg']}; }}
    DataTable > .datatable--header {{ background: {C['bg']}; color: {C['dim']}; text-style: bold; }}
    DataTable > .datatable--cursor {{ background: #153354; }}
    #detail {{
        height: auto; max-height: 12; margin: 0 1; padding: 1 2;
        border: round #1c2a3a; border-title-color: {C['accent']}; background: #0c121a;
    }}
    #empty {{ padding: 2 4; color: {C['dim']}; display: none; }}
    #empty.on {{ display: block; }}
    """ + work_css(C)
    BINDINGS = [
        Binding("q", "quit", "Salir"),
        Binding("f", "focus_agent", "Ir a su ventana (f/Enter)"),
        Binding("w", "work", "Modo Work"),
        Binding("a", "toggle_ended", "Mostrar cerradas"),
        Binding("r", "refresh", "Refrescar"),
        Binding("j", "cursor_down", show=False),
        Binding("k", "cursor_up", show=False),
    ]

    def __init__(self, work: bool = False) -> None:
        super().__init__()
        self.start_in_work = work
        self.monitor = Monitor()
        self.show_ended = False
        self.snapshot = Snapshot(agents=[], limits={})
        self.order: list[str] = []
        self.tick = 0

    def compose(self) -> ComposeResult:
        with Horizontal(id="top"):
            yield Static(Text("❄ ZERØ AGENTS"), id="brand")
            yield Static(id="summary")
            yield Static(id="limits")
        yield Static(id="alert")
        yield Static("No hay agentes abiertos. Abre Claude Code y aparecerán aquí.", id="empty")
        yield DataTable(id="agents", cursor_type="row", zebra_stripes=False)
        yield Static(id="detail")
        yield Footer()

    def on_mount(self) -> None:
        self.register_theme(FROST)
        self.theme = "frost"
        table = self.query_one(DataTable)
        for col in COLUMNS:
            table.add_column(col, key=col)
        self.query_one("#detail").border_title = "detalle"
        self.action_refresh()
        self.set_interval(1.0, self.action_refresh)
        if self.start_in_work:
            self.action_work()

    # ── datos ──────────────────────────────────────────────────────────────

    def action_refresh(self) -> None:
        self.tick += 1
        self.snapshot = self.monitor.snapshot(include_ended=self.show_ended)
        self._render_table()
        self._render_header()
        self._render_detail()

    def _status_cell(self, agent: Agent) -> Text:
        icon = SPINNER[self.tick % 4] if agent.status is Status.WORKING else fmt.ICONS[agent.status]
        label = agent.status.value.upper() if agent.status.needs_you else agent.status.value
        return Text(f"{icon} {label}", style=STATUS_STYLE[agent.status])

    def _row(self, a: Agent) -> list:
        dim = C["muted"] if not a.alive else None
        ctx = a.usage.context_pct
        ctx_style = C["red"] if (ctx or 0) >= 85 else C["yellow"] if (ctx or 0) >= 60 else C["cyan"]
        name = a.name + (f" +{a.subagents}" if a.subagents else "")
        mode_style = C["red"] if a.mode == "bypassPermissions" else C["lavender"]
        return [
            self._status_cell(a),
            Text(fmt.clip(name, 34), style=dim or f"bold {C['fg']}"),
            Text(fmt.PLATFORMS.get(a.platform, a.platform), style=C["dim"]),
            Text(fmt.place(a.cwd), style=dim or C["dim"]),
            Text(fmt.model(a.model) + (" ⚡" if a.fast else ""), style=dim or C["accent"]),
            Text(fmt.effort(a.effort), style=dim or C["fg"]),
            Text(fmt.mode(a.mode), style=dim or mode_style),
            Text(fmt.bar(ctx), style=dim or ctx_style),
            Text(fmt.tokens(a.usage.input_tokens + a.usage.output_tokens), style=dim or C["fg"]),
            Text(fmt.cost(a.usage.cost_usd), style=dim or C["fg"]),
            Text(fmt.ago(a.last_activity), style=C["dim"]),
        ]

    def _render_table(self) -> None:
        table = self.query_one(DataTable)
        selected = self.order[table.cursor_row] if self.order and table.cursor_row < len(self.order) else None
        table.clear()
        self.order = [a.id for a in self.snapshot.agents]
        for agent in self.snapshot.agents:
            table.add_row(*self._row(agent), key=agent.id)
        if selected in self.order:
            table.move_cursor(row=self.order.index(selected), animate=False)
        empty = not self.snapshot.agents
        self.query_one("#empty").set_class(empty, "on")
        table.display = not empty
        self.query_one("#detail").display = not empty

    def _render_header(self) -> None:
        agents = self.snapshot.agents
        live = [a for a in agents if a.alive]
        needs = [a for a in live if a.status.needs_you]
        working = sum(a.status is Status.WORKING for a in live)
        summary = Text(f"{len(live)} abiertos", style=C["fg"])
        summary.append(f"  ·  {working} trabajando", style=C["cyan"])
        summary.append(
            f"  ·  {len(needs)} te necesita{'n' if len(needs) != 1 else ''}",
            style=f"bold {C['red']}" if needs else C["dim"],
        )
        total = sum(a.usage.cost_usd or 0 for a in live)
        if total:
            summary.append(f"  ·  ≈{fmt.cost(total)} sesiones abiertas", style=C["dim"])
        self.query_one("#summary", Static).update(summary)

        limits = Text()
        for lim in self.snapshot.limits.values():
            limits.append_text(_limit_text("5h", lim.five_hour_pct, lim.five_hour_resets))
            limits.append("   ")
            limits.append_text(_limit_text("7d", lim.seven_day_pct, lim.seven_day_resets))
        self.query_one("#limits", Static).update(limits)

        alert = self.query_one("#alert", Static)
        if needs:
            lines = Text()
            for a in needs:
                lines.append("⚠ ", style=f"bold {C['red']}")
                lines.append(f"{a.name}: ", style=f"bold {C['fg']}")
                lines.append(f"{a.attention_msg or 'te necesita'}\n", style=C["yellow"])
            alert.update(lines)
        alert.set_class(bool(needs), "on")

    def _selected(self) -> Agent | None:
        table = self.query_one(DataTable)
        if not self.snapshot.agents or table.cursor_row >= len(self.snapshot.agents):
            return None
        return self.snapshot.agents[table.cursor_row]

    def _render_detail(self) -> None:
        a = self._selected()
        panel = self.query_one("#detail", Static)
        if not a:
            panel.update("")
            return
        panel.border_title = a.name
        u = a.usage
        t = Text()

        def field(label: str, value: str, style: str = C["fg"]) -> None:
            t.append(f"{label} ", style=C["dim"])
            t.append(value, style=style)
            t.append("   ")

        field("Estado", a.status.value, STATUS_STYLE[a.status])
        field("Plataforma", fmt.PLATFORMS.get(a.platform, a.platform))
        if a.pid:
            field("PID", str(a.pid))
        if a.subagents:
            field("Subagentes", str(a.subagents), C["cyan"])
        t.append("\n")
        field("Modelo", f"{fmt.model(a.model)} ({a.model or '?'})", C["accent"])
        field("Esfuerzo", fmt.effort(a.effort))
        field("Modo", fmt.mode(a.mode), C["lavender"])
        if a.fast:
            field("Rápido", "sí", C["yellow"])
        t.append("\n")
        size = f" de {fmt.tokens(u.context_size)}" if u.context_size else ""
        field("Contexto", f"{fmt.bar(u.context_pct, 10)}{size}")
        field("Tokens", f"{fmt.tokens(u.input_tokens)} entrada · {fmt.tokens(u.output_tokens)} salida"
              f" · {fmt.tokens(u.cache_read_tokens)} leídos de caché · {u.requests} peticiones")
        t.append("\n")
        field("Coste estimado", fmt.cost(u.cost_usd))
        field("Duración", fmt.duration(u.duration_ms))
        field("Carpeta", fmt.place(a.cwd))
        if a.attention_msg:
            t.append("\n")
            t.append("Necesita: ", style=f"bold {C['red']}")
            t.append(a.attention_msg, style=C["yellow"])
        if a.last_message:
            t.append("\n")
            t.append("Última respuesta: ", style=C["dim"])
            t.append(a.last_message, style=C["fg"])
        panel.update(t)

    # ── acciones ───────────────────────────────────────────────────────────

    def on_data_table_row_highlighted(self, _event: DataTable.RowHighlighted) -> None:
        self._render_detail()

    def action_cursor_down(self) -> None:
        self.query_one(DataTable).action_cursor_down()

    def action_cursor_up(self) -> None:
        self.query_one(DataTable).action_cursor_up()

    def action_work(self) -> None:
        self.push_screen(WorkScreen(C, self.monitor))

    def action_toggle_ended(self) -> None:
        self.show_ended = not self.show_ended
        self.notify("Mostrando también sesiones cerradas (24 h)" if self.show_ended else "Solo sesiones abiertas")
        self.action_refresh()

    def action_focus_agent(self) -> None:
        agent = self._selected()
        error = focus_pid(agent.pid if agent and agent.alive else None)
        if error:
            self.notify(error, severity="warning")

    def on_data_table_row_selected(self, _event: DataTable.RowSelected) -> None:
        self.action_focus_agent()


def run(work: bool = False) -> None:
    ZeroAgents(work=work).run()


if __name__ == "__main__":
    run()
