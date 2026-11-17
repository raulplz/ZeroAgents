//! Esqueleto de la app: lateral, pestañas, eventos del backend y teclado.

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use zeroagents_core::model::Board;
use zeroagents_core::theme::Theme;

use crate::tauri::{self, NoArgs};
use crate::util::{Toast, apply_theme};
use crate::{mensajes, sesiones, work};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sesiones,
    Mensajes,
    Work,
}

/// Estado compartido por todas las vistas (todo son señales, así que es `Copy`).
#[derive(Clone, Copy)]
pub struct Ctx {
    pub board: RwSignal<Option<Board>>,
    pub selected: RwSignal<Option<String>>,
    pub tab: RwSignal<Tab>,
    pub theme: RwSignal<Theme>,
    pub toast: Toast,
}

impl Ctx {
    pub fn get() -> Ctx {
        expect_context::<Ctx>()
    }

    /// Igual que `f`/Enter en el TUI: salta a la ventana del agente.
    pub fn focus(self, pid: Option<u32>) {
        #[derive(Serialize)]
        struct Args {
            pid: Option<u32>,
        }
        spawn_local(async move {
            match tauri::invoke::<_, ()>("focus_agent", &Args { pid }).await {
                Ok(()) => self.toast.show("VENTANA ENFOCADA", false),
                Err(e) => self.toast.show(e, true),
            }
        });
    }

    pub fn toggle_ended(self) {
        #[derive(Serialize)]
        struct Args {
            show: bool,
        }
        let show = !self.board.with_untracked(|b| b.as_ref().is_some_and(|b| b.show_ended));
        spawn_local(async move {
            if let Ok(board) = tauri::invoke::<_, Board>("set_show_ended", &Args { show }).await {
                self.board.set(Some(board));
            }
        });
    }

    /// Mueve la selección `delta` filas en la lista de sesiones.
    fn move_selection(self, delta: i32) {
        let ids: Vec<String> = self.board.with_untracked(|b| {
            b.as_ref().map(|b| b.snapshot.agents.iter().map(|a| a.id.clone()).collect()).unwrap_or_default()
        });
        if ids.is_empty() {
            return;
        }
        let current = self.selected.get_untracked().and_then(|s| ids.iter().position(|i| *i == s)).unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize;
        self.selected.set(Some(ids[next].clone()));
    }

    fn focus_selected(self) {
        let pid = self.board.with_untracked(|b| {
            let sel = self.selected.get_untracked();
            b.as_ref()
                .and_then(|b| b.snapshot.agents.iter().find(|a| Some(&a.id) == sel.as_ref()))
                .filter(|a| a.alive)
                .and_then(|a| a.pid)
        });
        self.focus(pid);
    }
}

#[component]
pub fn App() -> impl IntoView {
    let ctx = Ctx {
        board: RwSignal::new(None),
        selected: RwSignal::new(None),
        tab: RwSignal::new(Tab::Sesiones),
        theme: RwSignal::new(Theme::default()),
        toast: Toast(RwSignal::new(None)),
    };
    provide_context(ctx);

    // Primer dato a mano; después llegan solos por eventos.
    spawn_local(async move {
        if let Ok(theme) = tauri::invoke::<_, Theme>("theme", &NoArgs {}).await {
            ctx.theme.set(theme);
        }
        if let Ok(board) = tauri::invoke::<_, Board>("board", &NoArgs {}).await {
            ctx.board.set(Some(board));
        }
        tauri::listen("board", move |board: Board| ctx.board.set(Some(board))).await;
        tauri::listen("theme", move |theme: Theme| ctx.theme.set(theme)).await;
    });
    Effect::new(move |_| ctx.theme.with(apply_theme));

    // Si no hay selección (o desaparece la sesión), la primera de la lista.
    Effect::new(move |_| {
        ctx.board.with(|b| {
            let Some(b) = b else { return };
            let sel = ctx.selected.get_untracked();
            if !b.snapshot.agents.iter().any(|a| Some(&a.id) == sel.as_ref()) {
                ctx.selected.set(b.snapshot.agents.first().map(|a| a.id.clone()));
            }
        })
    });

    let keys = window_event_listener(ev::keydown, move |e| {
        if e.ctrl_key() || e.alt_key() || e.meta_key() {
            return;
        }
        let on_sessions = ctx.tab.get_untracked() == Tab::Sesiones;
        match e.key().as_str() {
            "j" | "ArrowDown" if on_sessions => ctx.move_selection(1),
            "k" | "ArrowUp" if on_sessions => ctx.move_selection(-1),
            "f" | "Enter" if on_sessions => ctx.focus_selected(),
            "a" if on_sessions => ctx.toggle_ended(),
            "1" | "s" => ctx.tab.set(Tab::Sesiones),
            "2" | "m" => ctx.tab.set(Tab::Mensajes),
            "3" | "w" => ctx.tab.set(Tab::Work),
            _ => return,
        }
        e.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let needs = move || {
        ctx.board.with(|b| {
            b.as_ref().map_or(0, |b| b.snapshot.agents.iter().filter(|a| a.alive && a.status.needs_you()).count())
        })
    };

    view! {
        <div class="fondo-rejilla" aria-hidden="true"></div>
        <div class="kanji" aria-hidden="true">{move || ctx.theme.with(|t| t.kanji.clone())}</div>
        <div class="marco">
            <aside class="lateral">
                <div class="marca" title="ZeroAgents">
                    <span class="marca-logo">"ZERØ"<span>"//"</span>"AGENTS"</span>
                    <span class="marca-sub">"SYS://AGENTS · v0.4"</span>
                </div>
                <nav>
                    <NavButton tab=Tab::Sesiones label="Sesiones" key="1" icon=ICON_SESIONES badge=Signal::derive(needs) />
                    <NavButton tab=Tab::Mensajes label="Mensajes" key="2" icon=ICON_MENSAJES badge=Signal::derive(|| 0) />
                    <NavButton tab=Tab::Work label="Modo Work" key="3" icon=ICON_WORK badge=Signal::derive(|| 0) />
                </nav>
                <Pie />
            </aside>
            <main>
                {move || match ctx.tab.get() {
                    Tab::Sesiones => sesiones::Sesiones().into_any(),
                    Tab::Mensajes => mensajes::Mensajes().into_any(),
                    Tab::Work => work::Work().into_any(),
                }}
            </main>
        </div>
        {move || ctx.toast.0.get().map(|(text, error)| view! { <div class="toast" class:error=error>{text}</div> })}
    }
}

const ICON_SESIONES: &str = "M3 4h18v6H3zM3 14h18v6H3zM7 7h.01M7 17h.01";
const ICON_MENSAJES: &str = "M4 5h16v11H9l-5 4zM8 9h8M8 12h5";
const ICON_WORK: &str = "M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12L4 7.5";

#[component]
fn NavButton(tab: Tab, label: &'static str, key: &'static str, icon: &'static str, badge: Signal<usize>) -> impl IntoView {
    let ctx = Ctx::get();
    view! {
        <button class:activa=move || ctx.tab.get() == tab on:click=move |_| ctx.tab.set(tab)>
            <svg viewBox="0 0 24 24"><path d=icon /></svg>
            <span>{label}</span>
            {move || {
                let n = badge.get();
                if n > 0 { view! { <b class="insignia">{n}</b> }.into_any() } else { view! { <kbd>{key}</kbd> }.into_any() }
            }}
        </button>
    }
}

#[component]
fn Pie() -> impl IntoView {
    let ctx = Ctx::get();
    let open_tui = move |_| {
        #[derive(Serialize)]
        struct Args {
            work: bool,
        }
        let work = ctx.tab.get_untracked() == Tab::Work;
        spawn_local(async move {
            if let Err(e) = tauri::invoke::<_, ()>("open_terminal", &Args { work }).await {
                ctx.toast.show(e, true);
            }
        });
    };
    let notifier = move || {
        ctx.board.with(|b| match b.as_ref().map(|b| b.notifier.as_str()) {
            Some("hooks") => "HOOKS",
            Some(_) => "ESTA APP",
            None => "—",
        })
    };
    view! {
        <div class="lateral-pie">
            <div>
                "TEMA · "
                <span class="glifo">{move || ctx.theme.with(|t| t.glifo.clone().unwrap_or_else(|| "◢".into()))}</span>
                <b>{move || ctx.theme.with(|t| t.nombre.to_uppercase())}</b>
            </div>
            <div>"AVISOS · " <b>{notifier}</b></div>
            <div>"FUENTE · " <b>"CLAUDE CODE"</b></div>
            <button class="boton fantasma mini" on:click=open_tui title="Abre el panel de terminal (zeroagents)">
                "PANEL TERMINAL"
            </button>
        </div>
    }
}
