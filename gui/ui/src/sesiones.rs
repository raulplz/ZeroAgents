//! Vista principal: cifras, límites 5 h / 7 d, quién te necesita, lista y detalle.

use leptos::prelude::*;
use zeroagents_core::fmt;
use zeroagents_core::model::{Agent, Board, Limits, Status};

use crate::app::Ctx;
use crate::util::clock;

fn status_icon(s: Status) -> &'static str {
    match s {
        Status::Permission | Status::Question => "●",
        Status::Working => "◐",
        Status::YourTurn => "○",
        Status::Unknown => "?",
        Status::Ended => "·",
    }
}

fn status_class(s: Status) -> &'static str {
    match s {
        Status::Permission => "estado permission",
        Status::Question => "estado question",
        Status::Working => "estado working",
        Status::YourTurn => "estado your_turn",
        Status::Unknown => "estado unknown",
        Status::Ended => "estado ended",
    }
}

fn status_badge(s: Status) -> impl IntoView + use<> {
    view! { <span class=status_class(s)><i>{status_icon(s)}</i>{s.label().to_uppercase()}</span> }
}

fn level_class(pct: Option<f64>) -> &'static str {
    match fmt::level(pct) {
        2 => "n2",
        1 => "n1",
        _ => "",
    }
}

/// Barra con brillo; `pct` de 0 a 100.
fn bar(pct: Option<f64>, fine: bool) -> impl IntoView + use<> {
    let width = format!("{:.1}%", pct.unwrap_or(0.0).clamp(0.0, 100.0));
    view! {
        <div class=format!("pista {}{}", level_class(pct), if fine { " fina" } else { "" })>
            <i style:width=width></i>
        </div>
    }
}

#[component]
pub fn Sesiones() -> impl IntoView {
    let ctx = Ctx::get();
    move || {
        ctx.board.with(|board| match board {
            None => view! { <div class="vacio"><b>"CONECTANDO"</b>"Leyendo el estado de Claude Code…"</div> }.into_any(),
            Some(b) => panel(b, ctx).into_any(),
        })
    }
}

fn panel(b: &Board, ctx: Ctx) -> impl IntoView + use<> {
    let agents = &b.snapshot.agents;
    let live: Vec<&Agent> = agents.iter().filter(|a| a.alive).collect();
    let needs: Vec<&Agent> = live.iter().copied().filter(|a| a.status.needs_you()).collect();
    let working = live.iter().filter(|a| a.status == Status::Working).count();
    let cost: f64 = live.iter().filter_map(|a| a.usage.cost_usd).sum();
    let limits = b.snapshot.limits.values().next().cloned().unwrap_or_default();
    let now = b.now;

    view! {
        <header class="cabecera">
            <div>
                <div class="pre">"SYS://MONITOR · CLAUDE CODE"</div>
                <h1>"Sesiones"</h1>
            </div>
            <div class="lado"><span><span class="pulso"></span>"EN VIVO · 1 S"</span><span>{clock(Some(now), now)}</span></div>
        </header>

        <section class="resumen">
            <div class="panel dato principal">
                <div class="k">"Abiertas"</div>
                <div class="v">{live.len()}</div>
                <div class="s">{format!("{} trabajando", working)}</div>
            </div>
            <div class=if needs.is_empty() { "panel dato" } else { "panel dato alarma" }>
                <div class="k">"Te necesitan"</div>
                <div class="v">{needs.len()}</div>
                <div class="s">{if needs.is_empty() { "nada pendiente" } else { "permiso o pregunta" }}</div>
            </div>
            <div class="panel dato">
                <div class="k">"Coste estimado"</div>
                <div class="v">{if cost > 0.0 { format!("${cost:.2}") } else { "—".into() }}</div>
                <div class="s">"precio de lista · abiertas"</div>
            </div>
            {limits_panel(&limits, now)}
        </section>

        {(!needs.is_empty()).then(|| alert(&needs, ctx))}

        <h2 class="seccion">
            "Agentes " <b>{agents.len()}</b>
            <span class=if b.show_ended { "chip on" } else { "chip" } on:click=move |_| ctx.toggle_ended()>
                <i></i>"Ver cerradas (24 h)"
            </span>
        </h2>
        <div class="sesiones-zona">
            {list(b, ctx)}
            {detail(b, ctx)}
        </div>
        <div class="atajos">
            <kbd>"j/k"</kbd>"moverse · " <kbd>"f"</kbd><kbd>"Enter"</kbd>"ir a su ventana · "
            <kbd>"a"</kbd>"cerradas · " <kbd>"1-3"</kbd>"vistas"
        </div>
    }
}

fn limits_panel(l: &Limits, now: f64) -> impl IntoView + use<> {
    let row = |label: &'static str, pct: Option<f64>, resets: Option<f64>| {
        view! {
            <div class=format!("limite {}", level_class(pct))>
                <div class="k">{label}<b>{fmt::pct(pct)}</b></div>
                {bar(pct, false)}
                <div class="r">{resets.map_or_else(|| "—".into(), |r| format!("RENUEVA {}", clock(Some(r), now)))}</div>
            </div>
        }
    };
    view! {
        <div class="panel brillo limites">
            {row("LÍMITE 5 H", l.five_hour_pct, l.five_hour_resets)}
            {row("LÍMITE 7 D", l.seven_day_pct, l.seven_day_resets)}
        </div>
    }
}

fn alert(needs: &[&Agent], ctx: Ctx) -> impl IntoView + use<> {
    let rows = needs
        .iter()
        .map(|a| {
            let pid = a.pid;
            let tag = if a.status == Status::Question { "tag q" } else { "tag" };
            view! {
                <div class="fila">
                    <span class=tag>{a.status.label().to_uppercase()}</span>
                    <div class="que">
                        <b>{a.name.clone()}</b>
                        <span>{a.attention_msg.clone().unwrap_or_else(|| "Te necesita".into())}</span>
                    </div>
                    <button class="boton rojo mini" on:click=move |_| ctx.focus(pid)>"Ir a su ventana"</button>
                </div>
            }
        })
        .collect_view();
    view! { <section class="panel alerta">{rows}</section> }
}

fn list(b: &Board, ctx: Ctx) -> impl IntoView + use<> {
    if b.snapshot.agents.is_empty() {
        return view! {
            <div class="panel vacio"><b>"SIN AGENTES"</b>"No hay sesiones abiertas. Abre Claude Code y aparecerán aquí."</div>
        }
        .into_any();
    }
    let selected = ctx.selected.get();
    let rows = b
        .snapshot
        .agents
        .iter()
        .map(|a| {
            let id = a.id.clone();
            let pid = a.pid.filter(|_| a.alive);
            let ctxp = a.usage.context_pct;
            let name = if a.subagents > 0 { format!("{} +{}", a.name, a.subagents) } else { a.name.clone() };
            let mode_class = if a.mode.as_deref() == Some("bypassPermissions") { "c modo peligro ancho" } else { "c modo ancho" };
            let mut class = String::from("fila-s");
            if selected.as_ref() == Some(&a.id) {
                class.push_str(" sel");
            }
            if a.status.needs_you() {
                class.push_str(" urge");
            }
            if !a.alive {
                class.push_str(" cerrada");
            }
            view! {
                <div class=class on:click=move |_| ctx.selected.set(Some(id.clone())) on:dblclick=move |_| ctx.focus(pid)>
                    <div>{status_badge(a.status)}</div>
                    <div class="nombre">
                        <b>{name}</b>
                        <small>{fmt::place(&a.cwd, &b.home)}</small>
                    </div>
                    <div class="c modelo">{fmt::model(a.model.as_deref())}{a.fast.then_some(" ⚡")}</div>
                    <div class="c ancho">{fmt::effort(a.effort.as_deref())}</div>
                    <div class=mode_class>{fmt::mode(a.mode.as_deref())}</div>
                    <div class=format!("ctx {}", level_class(ctxp))>{fmt::pct(ctxp)}{bar(ctxp, true)}</div>
                    <div class="c num medio">{fmt::tokens(a.usage.input_tokens + a.usage.output_tokens)}</div>
                    <div class="c num">{fmt::cost(a.usage.cost_usd)}</div>
                    <div class="c num tenue medio">{fmt::ago(a.last_activity, b.now)}</div>
                </div>
            }
        })
        .collect_view();
    view! {
        <div class="panel tabla">
            <div class="fila-s cab">
                <div>"Estado"</div><div>"Agente · carpeta"</div><div>"Modelo"</div><div class="ancho">"Esfuerzo"</div>
                <div class="ancho">"Modo"</div><div>"Contexto"</div><div class="num medio">"Tokens"</div>
                <div class="num">"Coste"</div><div class="num medio">"Hace"</div>
            </div>
            {rows}
        </div>
    }
    .into_any()
}

fn detail(b: &Board, ctx: Ctx) -> Option<impl IntoView + use<>> {
    let selected = ctx.selected.get();
    let a = b.snapshot.agents.iter().find(|a| Some(&a.id) == selected.as_ref())?;
    let u = &a.usage;
    let pid = a.pid.filter(|_| a.alive);
    let size = u.context_size.map(|s| format!(" de {}", fmt::tokens(s))).unwrap_or_default();
    let field = |k: &'static str, v: String| {
        let title = v.clone();
        view! { <div>{k}<b title=title>{v}</b></div> }
    };
    Some(view! {
        <section class=if a.status.needs_you() { "panel brillo detalle urge" } else { "panel brillo detalle" }>
            <div class="meta">
                {status_badge(a.status)}
                <span class="etiqueta">{fmt::platform(&a.platform).to_string()}</span>
            </div>
            <h3>{a.name.clone()}</h3>
            <div class="ruta">{fmt::place(&a.cwd, &b.home)}</div>
            <div class="datos">
                <div>"Modelo"<b title=a.model.clone().unwrap_or_default()>{fmt::model(a.model.as_deref())}{a.fast.then_some(" ⚡")}</b></div>
                {field("Esfuerzo", fmt::effort(a.effort.as_deref()))}
                {field("Modo", fmt::mode(a.mode.as_deref()))}
                <div>"Contexto"<b>{fmt::pct(u.context_pct)}<small>{size}</small></b>{bar(u.context_pct, true)}</div>
                {field("Entrada", fmt::tokens(u.input_tokens))}
                {field("Salida", fmt::tokens(u.output_tokens))}
                {field("Caché leída", fmt::tokens(u.cache_read_tokens))}
                {field("Peticiones", u.requests.to_string())}
                {field("Coste estimado", fmt::cost(u.cost_usd))}
                {field("Duración", fmt::duration(u.duration_ms))}
                {field("Actividad", format!("hace {}", fmt::ago(a.last_activity, b.now)))}
                {field("Subagentes", a.subagents.to_string())}
                {field("PID", a.pid.map_or_else(|| "—".into(), |p| p.to_string()))}
                {field("Id", a.id.chars().take(8).collect())}
            </div>
            {a.attention_msg.clone().map(|m| view! { <div class="bloque peligro"><div class="k">"NECESITA"</div><p>{m}</p></div> })}
            {a.last_message.clone().map(|m| view! { <div class="bloque"><div class="k">"ÚLTIMA RESPUESTA"</div><p>{m}</p></div> })}
            <div class="acciones">
                <button class=if a.status.needs_you() { "boton rojo" } else { "boton" } disabled=pid.is_none() on:click=move |_| ctx.focus(pid)>
                    "Ir a su ventana" <kbd>"F"</kbd>
                </button>
            </div>
        </section>
    })
}
