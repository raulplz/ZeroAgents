//! Ayudante de las pruebas de compatibilidad con la TUI de Python
//! (`tests/test_compat.py`). Usa el núcleo real sobre el HOME/XDG del entorno.
//!
//!   compat_helper register DIR NAME [N]   registra proyectos (N veces: DIR-i / NAME-i)
//!   compat_helper projects                lista los proyectos registrados
//!   compat_helper collect                 agentes que ve el adaptador de Claude Code (JSON)

use std::path::PathBuf;

use zeroagents_core::claude_code::ClaudeCode;
use zeroagents_core::{paths, work};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let state = paths::state_dir();
    match args.first().map(String::as_str) {
        Some("register") => {
            let (base, name) = (&args[1], &args[2]);
            match args.get(3).and_then(|n| n.parse::<usize>().ok()) {
                Some(n) => {
                    for i in 0..n {
                        work::register(&state, &PathBuf::from(format!("{base}-{i}")), &format!("{name}-{i}"))
                            .expect("registrar");
                    }
                }
                None => work::register(&state, &PathBuf::from(base), name).expect("registrar"),
            }
        }
        Some("register-dir") => {
            // Alta como la del formulario de la GUI: carpeta con plan, nombre opcional.
            let name = args.get(2).map(String::as_str);
            println!("{}", work::register_existing(&state, &PathBuf::from(&args[1]), name).expect("alta"));
        }
        Some("projects") => {
            let list: Vec<String> =
                work::projects(&state).iter().map(|p| p.to_string_lossy().into_owned()).collect();
            println!("{}", serde_json::json!(list));
        }
        Some("collect") => {
            let (agents, _) = ClaudeCode::new().collect(true);
            let out: Vec<_> = agents
                .iter()
                .map(|a| {
                    serde_json::json!({"id": a.id, "status": a.status.label(), "model": a.model,
                        "effort": a.effort, "cost": a.usage.cost_usd, "context_pct": a.usage.context_pct,
                        "cwd": a.cwd, "last_message": a.last_message})
                })
                .collect();
            println!("{}", serde_json::json!(out));
        }
        _ => {
            eprintln!("uso: compat_helper register|projects|collect");
            std::process::exit(2);
        }
    }
}
