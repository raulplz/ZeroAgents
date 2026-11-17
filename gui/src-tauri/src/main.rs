//! Backend de la app de escritorio de ZeroAgents (Tauri 2).
//!
//! - Un hilo lee el estado cada segundo con el núcleo (`zeroagents-core`) y
//!   emite el evento `board` a la interfaz.
//! - Cada 2 s mira si cambió `~/.config/zerolayer/theme.json` y emite `theme`.
//! - Los comandos (`invoke`) sirven lo que la interfaz pide bajo demanda.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use zeroagents_core::messages::{self, Message};
use zeroagents_core::model::Board;
use zeroagents_core::monitor::Monitor;
use zeroagents_core::notify::{self, Notifier};
use zeroagents_core::theme::{Theme, watch};
use zeroagents_core::work::{self, NewProject, ProjectView};
use zeroagents_core::{claude_code, focus, paths, store};

struct App {
    monitor: Mutex<Monitor>,
    show_ended: AtomicBool,
}

impl App {
    fn board(&self) -> Board {
        let show_ended = self.show_ended.load(Ordering::Relaxed);
        let snapshot = self.monitor.lock().map(|mut m| m.snapshot(show_ended)).unwrap_or_default();
        let notifier = if notify::hooks_notify(&paths::claude_dir()) { "hooks" } else { "app" };
        Board {
            snapshot,
            now: store::now(),
            home: paths::home().to_string_lossy().into(),
            show_ended,
            notifier: notifier.into(),
        }
    }
}

type Shared<'a> = State<'a, Arc<App>>;

#[tauri::command]
async fn board(app: Shared<'_>) -> Result<Board, String> {
    Ok(app.board())
}

#[tauri::command]
async fn set_show_ended(app: Shared<'_>, show: bool) -> Result<Board, String> {
    app.show_ended.store(show, Ordering::Relaxed);
    Ok(app.board())
}

/// Igual que `f`/Enter en el panel de terminal.
#[tauri::command]
async fn focus_agent(pid: Option<u32>) -> Result<(), String> {
    focus::focus_pid(pid)
}

#[tauri::command]
fn theme() -> Theme {
    watch::load()
}

/// Mensajes entre agentes de las últimas `hours` horas (0 = todos).
#[tauri::command]
async fn agent_messages(hours: f64) -> Result<Vec<Message>, String> {
    let dir = paths::platform_dir(claude_code::PLATFORM);
    let since = (hours > 0.0).then(|| store::now() - hours * 3600.0);
    let mut list = messages::load(&messages::sources(&dir), since);
    // La interfaz solo enseña los últimos; los textos van íntegros.
    let skip = list.len().saturating_sub(200);
    Ok(list.split_off(skip))
}

#[tauri::command]
async fn work_projects(app: Shared<'_>) -> Result<Vec<ProjectView>, String> {
    let sessions = app.monitor.lock().map(|mut m| m.snapshot(true).agents).unwrap_or_default();
    let claude = paths::claude_dir();
    Ok(work::projects(&paths::state_dir())
        .iter()
        .filter_map(|dir| work::build(&claude, dir, &sessions))
        .collect())
}

/// El CLI de Python: en el PATH o en `~/.local/bin` (el escritorio no siempre
/// hereda el PATH de la shell).
fn zeroagents_cli() -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join("zeroagents"))
        .find(|p| p.is_file());
    from_path.or_else(|| Some(paths::home().join(".local/bin/zeroagents")).filter(|p| p.is_file()))
}

/// Arranca el líder de un proyecto con `zeroagents work start` (Python).
#[tauri::command]
async fn work_start(dir: String) -> Result<String, String> {
    let cli = zeroagents_cli().ok_or("No encuentro el comando `zeroagents` (Python).")?;
    if !Path::new(&dir).join(work::PLAN_FILE).exists() {
        return Err("Esa carpeta no tiene plan del modo Work.".into());
    }
    let out = Command::new(cli)
        .args(["work", "start", &dir])
        .output()
        .map_err(|e| format!("No he podido ejecutar zeroagents: {e}"))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() { Ok(text(&out.stdout)) } else { Err(text(&out.stderr)) }
}

/// `~` al principio de una ruta que escribe el usuario.
fn expand(path: &str) -> PathBuf {
    let path = path.trim();
    match path.strip_prefix("~/") {
        Some(rest) => paths::home().join(rest),
        None if path == "~" => paths::home(),
        None => PathBuf::from(path),
    }
}

/// Alta de un proyecto que ya tiene plan: lo registra (`work::register`).
#[tauri::command]
async fn work_register(dir: String, name: String) -> Result<String, String> {
    if dir.trim().is_empty() {
        return Err("Indica la carpeta del proyecto.".into());
    }
    let name = work::register_existing(&paths::state_dir(), &expand(&dir), Some(&name))?;
    Ok(format!("PROYECTO AÑADIDO · {name}"))
}

/// Proyecto nuevo, como el formulario de la terminal: `zeroagents work new`
/// (el arquitecto tarda de 30 s a unos minutos), y después se registra.
#[tauri::command]
async fn work_new(values: NewProject) -> Result<String, String> {
    if values.dir.trim().is_empty() {
        return Err("Indica la carpeta del proyecto.".into());
    }
    let dir = expand(&values.dir);
    let args = values.cli_args(&dir)?;
    let cli = zeroagents_cli().ok_or("No encuentro el comando `zeroagents` (Python).")?;
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::Write;
        let mut child = Command::new(cli)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("No he podido ejecutar zeroagents: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(values.prompt.as_bytes());
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if err.is_empty() { "El arquitecto ha fallado.".into() } else { err });
        }
        let name = work::register_existing(&paths::state_dir(), &dir, None)?;
        Ok(format!("EQUIPO LISTO · {name}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Abre el panel de terminal (`zeroagents`, o `zeroagents --work`).
#[tauri::command]
async fn open_terminal(work: bool) -> Result<(), String> {
    let cli = zeroagents_cli().ok_or("No encuentro el comando `zeroagents` (Python).")?;
    let mut args = vec!["--app-id=ZeroAgents".to_string(), "--title=ZeroAgents".into(), "--".into()];
    args.push(cli.to_string_lossy().into());
    if work {
        args.push("--work".into());
    }
    Command::new("xdg-terminal-exec")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No he podido abrir la terminal: {e}"))
}

/// Bucle de fondo: instantánea cada segundo, tema cada 2 s y avisos.
fn background(handle: AppHandle, app: Arc<App>) {
    thread::spawn(move || {
        let mut notifier = Notifier::default();
        let mut themes = watch::ThemeWatch::default();
        let mut tick = 0u64;
        loop {
            let board = app.board();
            if board.notifier == "app" {
                for alert in notifier.check(&board.snapshot.agents, board.now) {
                    notify::send(&alert.title, &alert.body, alert.urgent);
                }
            }
            let _ = handle.emit("board", &board);
            if tick.is_multiple_of(2)
                && let Some(theme) = themes.poll()
            {
                let _ = handle.emit("theme", &theme);
            }
            thread::sleep(Duration::from_secs(1));
            tick += 1;
        }
    });
}

fn main() {
    let app = Arc::new(App { monitor: Mutex::new(Monitor::new()), show_ended: AtomicBool::new(false) });
    tauri::Builder::default()
        .manage(app.clone())
        .invoke_handler(tauri::generate_handler![
            board,
            set_show_ended,
            focus_agent,
            theme,
            agent_messages,
            work_projects,
            work_start,
            work_register,
            work_new,
            open_terminal
        ])
        .setup(move |tauri_app| {
            background(tauri_app.handle().clone(), app.clone());
            if let Some(window) = tauri_app.get_webview_window("main") {
                let _ = window.set_title("ZeroAgents");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("no he podido arrancar ZeroAgents");
}
