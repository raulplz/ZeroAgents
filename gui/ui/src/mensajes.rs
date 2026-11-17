//! Mensajes entre agentes (encargos, entregas y mensajes) de las últimas 24 h.

use std::collections::HashSet;
use std::time::Duration;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use zeroagents_core::messages::Message;

use crate::tauri;
use crate::util::{clock, now};

const PREVIEW_LINES: usize = 6;

#[derive(Serialize)]
struct Args {
    hours: f64,
}

#[component]
pub fn Mensajes() -> impl IntoView {
    let list = RwSignal::new(None::<Vec<Message>>);
    let open = RwSignal::new(HashSet::<String>::new());
    let load = move || {
        spawn_local(async move {
            if let Ok(msgs) = tauri::invoke::<_, Vec<Message>>("agent_messages", &Args { hours: 24.0 }).await {
                list.set(Some(msgs));
            }
        })
    };
    load();
    if let Ok(handle) = set_interval_with_handle(load, Duration::from_secs(2)) {
        on_cleanup(move || handle.clear());
    }

    let body = move || {
        list.with(|l| match l {
            None => view! { <div class="vacio"><b>"CARGANDO"</b></div> }.into_any(),
            Some(l) if l.is_empty() => view! {
                <div class="panel vacio"><b>"SIN MENSAJES"</b>
                    "Cuando un agente delegue en un subagente o le escriba a otro, lo verás aquí con el texto íntegro."</div>
            }
            .into_any(),
            Some(l) => l.iter().rev().map(|m| card(m, open)).collect_view().into_any(),
        })
    };
    view! {
        <header class="cabecera">
            <div>
                <div class="pre">"SYS://MENSAJES · ÚLTIMAS 24 H"</div>
                <h1>"Mensajes entre agentes"</h1>
            </div>
            <div class="lado">{move || list.with(|l| l.as_ref().map_or(0, Vec::len))}" MENSAJES"</div>
        </header>
        <div class="mensajes">{body}</div>
    }
}

fn card(m: &Message, open: RwSignal<HashSet<String>>) -> impl IntoView + use<> {
    let key = format!("{}-{}", m.t, m.sesion.clone().unwrap_or_default());
    let lines: Vec<&str> = m.texto.trim_end().lines().collect();
    let long = lines.len() > PREVIEW_LINES;
    let is_open = open.with(|o| o.contains(&key));
    let text = if long && !is_open {
        format!("{}\n… ({} líneas más)", lines[..PREVIEW_LINES].join("\n"), lines.len() - PREVIEW_LINES)
    } else {
        m.texto.trim_end().to_string()
    };
    let toggle = move |_| {
        open.update(|o| {
            if !o.remove(&key) {
                o.insert(key.clone());
            }
        })
    };
    view! {
        <article class=format!("panel msg {}", m.kind)>
            <div class="cab">
                <span class="etiqueta" style="--c: var(--c)">{m.label().to_string()}</span>
                <b>{m.de.clone().unwrap_or_else(|| "?".into())}</b>"→"<b>{m.para.clone().unwrap_or_else(|| "?".into())}</b>
                {m.titulo.clone().map(|t| view! { <span>"· "{t}</span> })}
                <span class="t">{clock(Some(m.t), now())}</span>
            </div>
            <pre>{text}</pre>
            {long.then(|| view! {
                <button class="boton fantasma mini mas" on:click=toggle>{if is_open { "Ver menos" } else { "Ver completo" }}</button>
            })}
        </article>
    }
}
