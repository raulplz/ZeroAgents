//! Modo Work: proyectos con su equipo, tareas y fase (solo lectura).
//!
//! «Añadir proyecto» (formulario con los campos de la terminal) llama a
//! `work_new` (arquitecto de Python) o `work_register` (alta de una carpeta con
//! plan); arrancar el líder llama a `zeroagents work start`.

use std::time::Duration;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use zeroagents_core::fmt;
use zeroagents_core::work::{NewProject, ProjectView};

use crate::app::Ctx;
use crate::tauri::{self, NoArgs};

#[component]
pub fn Work() -> impl IntoView {
    let ctx = Ctx::get();
    let list = RwSignal::new(None::<Vec<ProjectView>>);
    let load = move || {
        spawn_local(async move {
            match tauri::invoke::<_, Vec<ProjectView>>("work_projects", &NoArgs {}).await {
                Ok(p) => list.set(Some(p)),
                Err(e) => ctx.toast.show(e, true),
            }
        })
    };
    load();
    if let Ok(handle) = set_interval_with_handle(load, Duration::from_secs(3)) {
        on_cleanup(move || handle.clear());
    }
    let adding = RwSignal::new(false);
    let open_tui = move |_| {
        #[derive(Serialize)]
        struct Args {
            work: bool,
        }
        spawn_local(async move {
            if let Err(e) = tauri::invoke::<_, ()>("open_terminal", &Args { work: true }).await {
                ctx.toast.show(e, true);
            }
        });
    };

    view! {
        <header class="cabecera">
            <div>
                <div class="pre">"SYS://WORK · EQUIPOS DE AGENTES"</div>
                <h1>"Modo Work"</h1>
            </div>
            <div class="lado">
                <button class="boton" on:click=move |_| adding.set(true)>"+ Añadir proyecto"</button>
                <button class="boton fantasma mini" on:click=open_tui title="Crear proyectos: tecla n en el panel de terminal">
                    "Nuevo proyecto (terminal)"
                </button>
            </div>
        </header>
        {move || adding.get().then(|| view! { <AddProject open=adding reload=Callback::new(move |_| load()) /> })}
        {move || list.with(|l| match l {
            None => view! { <div class="vacio"><b>"CARGANDO"</b></div> }.into_any(),
            Some(l) if l.is_empty() => view! {
                <div class="panel vacio"><b>"SIN PROYECTOS"</b>
                    "Crea uno con " <span class="mono">"zeroagents work new \"…\""</span> " o con la tecla n del modo Work en terminal."</div>
            }.into_any(),
            Some(l) => l.iter().map(|p| project(p, ctx)).collect_view().into_any(),
        })}
    }
}

fn project(p: &ProjectView, ctx: Ctx) -> impl IntoView + use<> {
    let pct = if p.total > 0 { p.done as f64 / p.total as f64 * 100.0 } else { 0.0 };
    let lead_pid = p.lead.as_ref().and_then(|l| l.pid);
    let needs = p.lead.as_ref().is_some_and(|l| l.status.needs_you());
    let dir = p.dir.clone();
    let start = move |_| {
        #[derive(Serialize)]
        struct Args {
            dir: String,
        }
        let dir = dir.clone();
        spawn_local(async move {
            match tauri::invoke::<_, String>("work_start", &Args { dir }).await {
                Ok(_) => ctx.toast.show("LÍDER ABIERTO EN UNA TERMINAL", false),
                Err(e) => ctx.toast.show(e, true),
            }
        });
    };
    let home = ctx.board.with_untracked(|b| b.as_ref().map(|b| b.home.clone()).unwrap_or_default());
    let lead_text = match &p.lead {
        Some(l) if l.status.needs_you() => format!("TE NECESITA · {}", l.attention_msg.clone().unwrap_or_default()),
        Some(l) => format!("LÍDER · {}", l.status.label().to_uppercase()),
        None => "SIN LÍDER ABIERTO".into(),
    };
    let members = p
        .members
        .iter()
        .map(|m| {
            let cls = match m.state.as_str() {
                "trabajando" => "f curso",
                "terminado" => "f hecha",
                _ => "f",
            };
            view! {
                <div class=cls>
                    <span class="marca-t">{if m.in_team { "◆" } else { "◇" }}</span>
                    <div><b>{m.name.clone()}</b><small>{format!("{} · {} · {}", m.title, m.model, fmt::effort(Some(&m.effort)))}</small>
                        {m.current.clone().map(|c| view! { <small>"▸ "{c}</small> })}</div>
                    <span class="etiqueta">{m.state.clone()}</span>
                </div>
            }
        })
        .collect_view();
    let tasks = p
        .tasks
        .iter()
        .map(|t| {
            let (cls, mark) = match t.status.as_str() {
                "hecha" => ("f hecha", "✓"),
                "en curso" => ("f curso", "◐"),
                _ if t.blocked => ("f bloq", "⧗"),
                _ => ("f", "·"),
            };
            view! {
                <div class=cls>
                    <span class="marca-t">{mark}</span>
                    <div><b>{t.subject.clone()}</b><small>{t.id.clone()}</small></div>
                    <span class="tenue mono">{t.owner.clone().unwrap_or_default()}</span>
                </div>
            }
        })
        .collect_view();
    view! {
        <section class="panel brillo proyecto">
            <div class="cab">
                <div>
                    <span class="etiqueta">{p.phase.clone()}</span>
                    <h3>{p.name.clone()}</h3>
                    <p class="res">{p.summary.clone()}</p>
                    <div class="ruta mono tenue">{fmt::place(&p.dir, &home)}</div>
                </div>
                <div class="acciones">
                    <button class="boton" on:click=start disabled=p.lead.is_some()>"Arrancar líder"</button>
                    <button class=if needs { "boton rojo" } else { "boton fantasma" } disabled=lead_pid.is_none()
                        on:click=move |_| ctx.focus(lead_pid)>"Ir al líder"</button>
                </div>
            </div>
            <div class="progreso">
                <div class="pista"><i style:width=format!("{pct:.0}%")></i></div>
                <span>{format!("{}/{} TAREAS", p.done, p.total)}</span>
            </div>
            <div class=if needs { "mono n2" } else { "mono tenue" } style="font-size:11px; letter-spacing:.12em; color: var(--c, inherit)">{lead_text}</div>
            <div class="columnas">
                <div>
                    <h2 class="seccion">"Equipo " <b>{p.members.len() + 1}</b></h2>
                    <div class="lista-w">
                        <div class="f"><span class="marca-t">"★"</span>
                            <div><b>"team-lead"</b><small>{format!("Product Owner · {} · {}", p.lead_model, fmt::effort(Some(&p.lead_effort)))}</small></div>
                            <span class="etiqueta">"líder"</span></div>
                        {members}
                    </div>
                </div>
                <div>
                    <h2 class="seccion">"Tareas " <b>{p.total}</b>{(!p.live_tasks).then_some(" · del plan")}</h2>
                    <div class="lista-w">{tasks}</div>
                </div>
            </div>
        </section>
    }
}

/// Formulario «Añadir proyecto»: o se diseña un equipo nuevo (los campos de la
/// terminal) o se registra una carpeta que ya tiene plan.
#[component]
fn AddProject(open: RwSignal<bool>, reload: Callback<()>) -> impl IntoView {
    let ctx = Ctx::get();
    let existing = RwSignal::new(false);
    let prompt = RwSignal::new(String::new());
    let dir = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let models = RwSignal::new("haiku,sonnet,opus".to_string());
    let architect = RwSignal::new("sonnet:medium".to_string());
    let perms = RwSignal::new("acceptEdits".to_string());
    let busy = RwSignal::new(false);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let result = if existing.get_untracked() {
                #[derive(Serialize)]
                struct Args {
                    dir: String,
                    name: String,
                }
                tauri::invoke::<_, String>("work_register", &Args { dir: dir.get_untracked(), name: name.get_untracked() }).await
            } else {
                #[derive(Serialize)]
                struct Args {
                    values: NewProject,
                }
                let arch = architect.get_untracked();
                let (model, effort) = arch.split_once(':').unwrap_or(("sonnet", "medium"));
                let values = NewProject {
                    prompt: prompt.get_untracked(),
                    dir: dir.get_untracked(),
                    models: models.get_untracked(),
                    architect_model: model.into(),
                    architect_effort: effort.into(),
                    permission_mode: perms.get_untracked(),
                };
                tauri::invoke::<_, String>("work_new", &Args { values }).await
            };
            busy.set(false);
            match result {
                Ok(msg) => {
                    ctx.toast.show(msg, false);
                    open.set(false);
                    reload.run(());
                }
                Err(e) => ctx.toast.show(e, true),
            }
        });
    };

    view! {
        <div class="velo" on:click=move |_| if !busy.get_untracked() { open.set(false) }>
            <form class="panel brillo formulario" on:click=|e| e.stop_propagation()
                on:submit=move |e| { e.prevent_default(); submit(()); }>
                <div class="pre">"SYS://WORK · ALTA"</div>
                <h2>"Añadir proyecto"</h2>
                <div class="chips">
                    <button type="button" class="chip" class:on=move || !existing.get() on:click=move |_| existing.set(false)><i></i>"Nuevo · arquitecto"</button>
                    <button type="button" class="chip" class:on=move || existing.get() on:click=move |_| existing.set(true)><i></i>"Carpeta con plan"</button>
                </div>
                {move || if existing.get() {
                    view! {
                        <label>"Carpeta del proyecto (con .zeroagents/team.json)"
                            <input prop:value=move || dir.get() on:input=move |e| dir.set(event_target_value(&e)) placeholder="~/proyectos/mi-proyecto" /></label>
                        <label>"Nombre (vacío = el del plan)"
                            <input prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e)) /></label>
                    }.into_any()
                } else {
                    view! {
                        <label>"Describe el proyecto: qué es, para quién, tecnologías o límites"
                            <textarea rows="5" prop:value=move || prompt.get() on:input=move |e| prompt.set(event_target_value(&e))></textarea></label>
                        <label>"Carpeta"
                            <input prop:value=move || dir.get() on:input=move |e| dir.set(event_target_value(&e)) placeholder="~/proyectos/mi-proyecto" /></label>
                        <label>"Modelos que puede usar el equipo (haiku, sonnet, opus, fable)"
                            <input prop:value=move || models.get() on:input=move |e| models.set(event_target_value(&e)) /></label>
                        <label>"Arquitecto de equipos"
                            <select prop:value=move || architect.get() on:change=move |e| architect.set(event_target_value(&e))>
                                <option value="sonnet:medium">"Rápido · Sonnet, esfuerzo medio (~30 s)"</option>
                                <option value="opus:high">"Calidad · Opus, esfuerzo alto (1-3 min, más caro)"</option>
                            </select></label>
                        <label>"Permisos del equipo al arrancar"
                            <select prop:value=move || perms.get() on:change=move |e| perms.set(event_target_value(&e))>
                                <option value="acceptEdits">"Aceptar ediciones de archivos (Bash sigue preguntando)"</option>
                                <option value="default">"Preguntar todo"</option>
                            </select></label>
                    }.into_any()
                }}
                <div class="acciones">
                    <button type="submit" class="boton" disabled=move || busy.get()>
                        {move || match (busy.get(), existing.get()) {
                            (true, false) => "Diseñando equipo…",
                            (true, true) => "Guardando…",
                            (false, false) => "Diseñar equipo",
                            (false, true) => "Añadir",
                        }}
                    </button>
                    <button type="button" class="boton fantasma" disabled=move || busy.get() on:click=move |_| open.set(false)>"Cancelar"</button>
                </div>
            </form>
        </div>
    }
}
