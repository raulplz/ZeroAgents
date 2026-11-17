//! Estado en vivo de los proyectos del modo Work (port de `work/live.py`, solo lectura).
//!
//! Junta el plan (`.zeroagents/team.json`), las sesiones abiertas (el líder y
//! si te necesita) y el estado interno de los equipos de Claude Code
//! (`~/.claude/teams/<equipo>/config.json` y `~/.claude/tasks/<equipo>/`). Ese
//! formato no está documentado, así que se lee de forma tolerante.
//!
//! Crear proyectos (el arquitecto) y arrancar el líder siguen en Python: la
//! GUI llama a `zeroagents work start`.

// Los tipos se comparten con el frontend; la lectura necesita `native`.
#![cfg_attr(not(feature = "native"), allow(dead_code, unused_imports))]

use std::collections::{HashMap, HashSet};
#[cfg(feature = "native")]
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Agent;
#[cfg(feature = "native")]
use crate::store;

pub const PLAN_FILE: &str = ".zeroagents/team.json";
pub const PHASES: [&str; 5] = ["requisitos", "arquitectura", "implementacion", "qa", "transversal"];
pub const PENDING: &str = "pendiente";
pub const RUNNING: &str = "en curso";
pub const DONE: &str = "hecha";

pub fn phase_label(phase: &str) -> &str {
    match phase {
        "requisitos" => "1 · Requisitos",
        "arquitectura" => "2 · Arquitectura",
        "implementacion" => "3 · Implementación",
        "qa" => "4 · Pruebas QA",
        "transversal" => "Transversal",
        other => other,
    }
}

fn norm_status(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "pending" | "todo" | "open" | "not_started" => PENDING.into(),
        "in_progress" | "in progress" | "in-progress" | "running" | "active" | "claimed" => RUNNING.into(),
        "completed" | "complete" | "done" | "finished" => DONE.into(),
        other => other.into(),
    }
}

// ── el plan ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlanFile {
    pub plan: Plan,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub project: PlanProject,
    pub lead: PlanLead,
    pub agents: Vec<PlanAgent>,
    pub tasks: Vec<PlanTask>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlanProject {
    pub name: String,
    pub summary: String,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlanLead {
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlanAgent {
    pub name: String,
    pub title: String,
    pub phase: String,
    pub model: String,
    pub effort: String,
    pub why: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlanTask {
    pub id: String,
    pub subject: String,
    pub owner: Option<String>,
    pub phase: String,
    pub blocked_by: Vec<String>,
}

#[cfg(feature = "native")]
pub fn load_plan(dir: &Path) -> Option<PlanFile> {
    store::read_json(&dir.join(PLAN_FILE))
}

/// Orden topológico estable (las dependencias primero).
pub fn task_order(tasks: &[PlanTask]) -> Vec<&PlanTask> {
    fn add<'a>(t: &'a PlanTask, by_id: &HashMap<&str, &'a PlanTask>, done: &mut HashSet<String>, out: &mut Vec<&'a PlanTask>) {
        if !done.insert(t.id.clone()) {
            return;
        }
        for dep in &t.blocked_by {
            if let Some(d) = by_id.get(dep.as_str()) {
                add(d, by_id, done, out);
            }
        }
        out.push(t);
    }
    let by_id: HashMap<&str, &PlanTask> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let (mut done, mut out) = (HashSet::new(), Vec::new());
    for t in tasks {
        add(t, &by_id, &mut done, &mut out);
    }
    out
}

// ── registro de proyectos ───────────────────────────────────────────────────

/// Versión actual de `work/projects.json`.
pub const REGISTRY_VERSION: u32 = 1;

/// `work/projects.json`. Lo que no se conoce (`extra`, y los campos de cada
/// proyecto, que van como `Value`) se conserva al reescribir.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub version: u32,
    pub projects: Vec<Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Registry {
    fn dir_of(item: &Value) -> Option<&str> {
        item.get("dir").and_then(Value::as_str)
    }
}

fn migrate_registry(reg: &mut Registry) -> bool {
    if reg.version >= REGISTRY_VERSION {
        return false;
    }
    reg.version = REGISTRY_VERSION;
    true
}

/// Apunta el proyecto el primero de la lista (igual que `live.register` en Python).
#[cfg(feature = "native")]
pub fn register(state: &Path, dir: &Path, name: &str) -> std::io::Result<()> {
    let key = dir.to_string_lossy().into_owned();
    crate::jsonfile::update::<Registry>(&state.join("work/projects.json"), migrate_registry, |reg| {
        let pos = reg.projects.iter().position(|p| Registry::dir_of(p) == Some(&key));
        let mut item = match pos {
            Some(i) => reg.projects.remove(i),
            None => Value::Object(Default::default()),
        };
        if !item.is_object() {
            item = Value::Object(Default::default());
        }
        let map = item.as_object_mut().expect("objeto");
        map.insert("dir".into(), key.clone().into());
        map.insert("name".into(), name.into());
        map.insert("added".into(), store::now().into());
        reg.projects.insert(0, item);
    })
    .map(|_| ())
}

/// Alta de un proyecto que ya tiene plan (`.zeroagents/team.json`): lo apunta
/// el primero de la lista. Sin `name` usa el del plan. Devuelve el nombre.
#[cfg(feature = "native")]
pub fn register_existing(state: &Path, dir: &Path, name: Option<&str>) -> Result<String, String> {
    let dir = dir.canonicalize().map_err(|_| format!("No existe la carpeta {}.", dir.display()))?;
    let plan = load_plan(&dir).ok_or("Esa carpeta no tiene plan del modo Work (falta .zeroagents/team.json).")?;
    let name = name.map(str::trim).filter(|n| !n.is_empty()).map(String::from).unwrap_or(plan.plan.project.name);
    let name = if name.is_empty() { dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name };
    register(state, &dir, &name).map_err(|e| format!("No he podido guardar el registro: {e}"))?;
    Ok(name)
}

pub const MODELS: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];
pub const PERMISSION_MODES: [&str; 4] = ["default", "acceptEdits", "plan", "auto"];

/// Los mismos campos que el formulario «Nuevo proyecto» de la terminal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewProject {
    pub prompt: String,
    pub dir: String,
    pub models: String,
    pub architect_model: String,
    pub architect_effort: String,
    pub permission_mode: String,
}

impl NewProject {
    /// Valida y arma los argumentos de `zeroagents work new` (el prompt va por stdin).
    pub fn cli_args(&self, dir: &Path) -> Result<Vec<String>, String> {
        if self.prompt.trim().is_empty() {
            return Err("Escribe primero el prompt del proyecto.".into());
        }
        let models: Vec<&str> = self.models.split(',').map(str::trim).filter(|m| !m.is_empty()).collect();
        let bad: Vec<&str> = models.iter().copied().filter(|m| !MODELS.contains(m)).collect();
        if models.is_empty() || !bad.is_empty() {
            let bad = if bad.is_empty() { "ninguno".to_string() } else { bad.join(", ") };
            return Err(format!("Modelos no válidos: {bad} (usa {}).", MODELS.join(", ")));
        }
        if !["sonnet", "opus"].contains(&self.architect_model.as_str()) || !["medium", "high"].contains(&self.architect_effort.as_str()) {
            return Err("Arquitecto no válido.".into());
        }
        if !PERMISSION_MODES.contains(&self.permission_mode.as_str()) {
            return Err("Modo de permisos no válido.".into());
        }
        Ok(vec![
            "work".into(), "new".into(), "-".into(),
            "--dir".into(), dir.to_string_lossy().into(),
            "--models".into(), models.join(","),
            "--architect-model".into(), self.architect_model.clone(),
            "--architect-effort".into(), self.architect_effort.clone(),
            "--permission-mode".into(), self.permission_mode.clone(),
        ])
    }
}

/// Proyectos registrados que siguen teniendo plan, del más reciente al más antiguo.
#[cfg(feature = "native")]
pub fn projects(state: &Path) -> Vec<PathBuf> {
    let reg: Registry = store::read_json(&state.join("work/projects.json")).unwrap_or_default();
    reg.projects
        .iter()
        .filter_map(Registry::dir_of)
        .map(PathBuf::from)
        .filter(|d| d.join(PLAN_FILE).exists())
        .collect()
}

// ── vistas ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskView {
    pub id: String,
    pub subject: String,
    pub owner: Option<String>,
    pub status: String,
    pub blocked_by: Vec<String>,
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemberView {
    pub name: String,
    pub title: String,
    pub phase: String,
    pub model: String,
    pub effort: String,
    pub why: String,
    pub in_team: bool,
    pub state: String,
    pub current: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectView {
    pub dir: String,
    pub name: String,
    pub summary: String,
    pub open_questions: Vec<String>,
    pub lead_model: String,
    pub lead_effort: String,
    /// Sesión del líder, si está abierta.
    pub lead: Option<Agent>,
    pub team_name: Option<String>,
    pub members: Vec<MemberView>,
    pub tasks: Vec<TaskView>,
    /// `true` si las tareas vienen del equipo real y no del plan.
    pub live_tasks: bool,
    pub phase: String,
    pub done: usize,
    pub total: usize,
}

fn first<'a>(d: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| d.get(*k)).find(|v| !v.is_null() && v.as_str() != Some(""))
}

fn as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn norm_task(d: &Value) -> Option<TaskView> {
    let subject = first(d, &["subject", "title", "content", "description"])?.as_str()?;
    let status = first(d, &["status", "state"]).and_then(as_text).unwrap_or_else(|| "pending".into());
    let deps = first(d, &["blockedBy", "blocked_by", "dependencies", "depends_on"])
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(as_text).collect())
        .unwrap_or_default();
    Some(TaskView {
        id: first(d, &["id", "taskId", "task_id"]).and_then(as_text).unwrap_or_else(|| "?".into()),
        subject: subject.trim().lines().next().unwrap_or("").to_string(),
        owner: first(d, &["owner", "assignee", "teammate", "assigned_to"]).and_then(as_text),
        status: norm_status(&status),
        blocked_by: deps,
        blocked: false,
    })
}

#[cfg(feature = "native")]
pub fn read_team_tasks(team_dir: &Path) -> Vec<TaskView> {
    let mut files: Vec<PathBuf> = fs::read_dir(team_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let mut tasks = Vec::new();
    for path in files {
        let Some(raw) = store::read_json::<Value>(&path) else { continue };
        let items: Vec<&Value> = match &raw {
            Value::Array(items) => items.iter().collect(),
            Value::Object(map) => match map.get("tasks") {
                Some(Value::Array(items)) => items.iter().collect(),
                _ => vec![&raw],
            },
            _ => continue,
        };
        tasks.extend(items.into_iter().filter_map(norm_task));
    }
    tasks
}

#[cfg(feature = "native")]
fn same_dir(a: &str, b: &Path) -> bool {
    let a = fs::canonicalize(a).unwrap_or_else(|_| PathBuf::from(a));
    let b = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    a == b
}

/// Equipo cuyo líder es una sesión abierta en la carpeta del proyecto.
#[cfg(feature = "native")]
pub fn find_team(claude: &Path, dir: &Path, sessions: &[Agent]) -> Option<String> {
    let teams = claude.join("teams");
    if !teams.is_dir() {
        return None;
    }
    let mut here: Vec<&Agent> = sessions.iter().filter(|a| !a.cwd.is_empty() && same_dir(&a.cwd, dir)).collect();
    here.sort_by(|a, b| b.last_activity.unwrap_or(0.0).total_cmp(&a.last_activity.unwrap_or(0.0)));
    for agent in &here {
        let candidate = format!("session-{}", agent.id.chars().take(8).collect::<String>());
        if teams.join(&candidate).is_dir() {
            return Some(candidate);
        }
    }
    // Respaldo: un config.json que mencione la carpeta o una sesión de aquí.
    let dir_text = dir.to_string_lossy();
    for entry in fs::read_dir(&teams).into_iter().flatten().flatten() {
        let text = fs::read_to_string(entry.path().join("config.json")).unwrap_or_default();
        if !text.is_empty() && (text.contains(dir_text.as_ref()) || here.iter().any(|a| text.contains(&a.id))) {
            return Some(entry.file_name().to_string_lossy().into());
        }
    }
    None
}

#[cfg(feature = "native")]
pub fn build(claude: &Path, dir: &Path, sessions: &[Agent]) -> Option<ProjectView> {
    let plan = load_plan(dir)?.plan;
    let mut view = ProjectView {
        dir: dir.to_string_lossy().into(),
        name: plan.project.name.clone(),
        summary: plan.project.summary.clone(),
        open_questions: plan.project.open_questions.clone(),
        lead_model: plan.lead.model.clone(),
        lead_effort: plan.lead.effort.clone(),
        ..ProjectView::default()
    };

    // Solo sesiones abiertas: la del arquitecto (`claude -p`) también corrió aquí y ya terminó.
    view.lead = sessions
        .iter()
        .filter(|a| a.alive && !a.cwd.is_empty() && same_dir(&a.cwd, dir))
        .max_by(|a, b| a.last_activity.unwrap_or(0.0).total_cmp(&b.last_activity.unwrap_or(0.0)))
        .cloned();
    view.team_name = find_team(claude, dir, sessions);

    let mut live_members = HashSet::new();
    if let Some(team) = &view.team_name {
        let cfg: Value = store::read_json(&claude.join("teams").join(team).join("config.json")).unwrap_or(Value::Null);
        for m in cfg.get("members").and_then(Value::as_array).into_iter().flatten() {
            if let Some(name) = first(m, &["name", "agentName"]).and_then(as_text) {
                live_members.insert(name);
            }
        }
        view.tasks = read_team_tasks(&claude.join("tasks").join(team));
        view.live_tasks = !view.tasks.is_empty();
    }
    if view.tasks.is_empty() {
        view.tasks = task_order(&plan.tasks)
            .into_iter()
            .map(|t| TaskView {
                id: t.id.clone(),
                subject: t.subject.clone(),
                owner: t.owner.clone(),
                status: PENDING.into(),
                blocked_by: t.blocked_by.clone(),
                blocked: false,
            })
            .collect();
    }

    let done: HashSet<String> = view.tasks.iter().filter(|t| t.status == DONE).map(|t| t.id.clone()).collect();
    for t in &mut view.tasks {
        t.blocked = t.status == PENDING && t.blocked_by.iter().any(|d| !done.contains(d));
    }

    for a in &plan.agents {
        let mine: Vec<&TaskView> = view.tasks.iter().filter(|t| t.owner.as_deref() == Some(a.name.as_str())).collect();
        let in_team = live_members.contains(&a.name);
        let running = mine.iter().find(|t| t.status == RUNNING);
        let (state, current) = if let Some(t) = running {
            ("trabajando", Some(t.subject.clone()))
        } else if view.team_name.is_none() {
            ("sin arrancar", None)
        } else if !mine.is_empty() && mine.iter().all(|t| t.status == DONE) {
            ("terminado", None)
        } else if mine.iter().any(|t| t.status == PENDING && !t.blocked) {
            ("con tarea lista", None)
        } else if mine.iter().any(|t| t.blocked) {
            ("esperando dependencias", None)
        } else if in_team {
            ("libre", None)
        } else {
            ("no creado", None)
        };
        view.members.push(MemberView {
            name: a.name.clone(),
            title: a.title.clone(),
            phase: a.phase.clone(),
            model: a.model.clone(),
            effort: a.effort.clone(),
            why: a.why.clone(),
            in_team,
            state: state.into(),
            current,
        });
    }

    view.done = view.tasks.iter().filter(|t| t.status == DONE).count();
    view.total = view.tasks.len();
    view.phase = current_phase(&plan, &view.tasks);
    Some(view)
}

/// Fase actual: la primera del ciclo con tareas sin terminar.
fn current_phase(plan: &Plan, tasks: &[TaskView]) -> String {
    let by_id: HashMap<&str, &str> = plan.tasks.iter().map(|t| (t.id.as_str(), t.phase.as_str())).collect();
    let by_subject: HashMap<String, &str> =
        plan.tasks.iter().map(|t| (t.subject.to_lowercase(), t.phase.as_str())).collect();
    let by_owner: HashMap<&str, &str> = plan.agents.iter().map(|a| (a.name.as_str(), a.phase.as_str())).collect();
    // El equipo real numera las tareas a su manera: id, luego título, luego responsable.
    let phase_of = |t: &TaskView| {
        by_id
            .get(t.id.as_str())
            .or_else(|| by_subject.get(&t.subject.to_lowercase()))
            .or_else(|| by_owner.get(t.owner.as_deref().unwrap_or("")))
            .copied()
    };
    for phase in PHASES {
        if tasks.iter().any(|t| phase_of(t) == Some(phase) && t.status != DONE) {
            return phase_label(phase).into();
        }
    }
    if !tasks.is_empty() && tasks.iter().all(|t| t.status == DONE) { "terminado".into() } else { "—".into() }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use crate::store::tempdir;
    use serde_json::json;

    fn plan_json() -> Value {
        json!({"plan": {
            "project": {"name": "Notas", "summary": "App de notas", "open_questions": ["¿SQLite?"]},
            "lead": {"model": "opus", "effort": "high"},
            "agents": [
                {"name": "analista", "title": "Requisitos", "phase": "requisitos", "model": "sonnet", "effort": "medium"},
                {"name": "dev", "title": "Desarrollo", "phase": "implementacion", "model": "sonnet", "effort": "high"}
            ],
            "tasks": [
                {"id": "T2", "subject": "Implementar", "owner": "dev", "phase": "implementacion", "blocked_by": ["T1"]},
                {"id": "T1", "subject": "Requisitos", "owner": "analista", "phase": "requisitos", "blocked_by": []}
            ]
        }})
    }

    #[test]
    fn sin_equipo_usa_las_tareas_del_plan_en_orden() {
        let root = tempdir("work-plan");
        let dir = root.join("notas");
        fs::create_dir_all(dir.join(".zeroagents")).unwrap();
        fs::write(dir.join(PLAN_FILE), plan_json().to_string()).unwrap();
        let view = build(&root.join("claude"), &dir, &[]).unwrap();
        assert_eq!(view.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["T1", "T2"]);
        assert!(!view.tasks[0].blocked && view.tasks[1].blocked);
        assert_eq!(view.phase, "1 · Requisitos");
        assert_eq!((view.done, view.total), (0, 2));
        assert!(view.members.iter().all(|m| m.state == "sin arrancar"));
    }

    #[test]
    fn con_equipo_lee_sus_tareas_de_forma_tolerante() {
        let root = tempdir("work-team");
        let claude = root.join("claude");
        let dir = root.join("notas");
        fs::create_dir_all(dir.join(".zeroagents")).unwrap();
        fs::write(dir.join(PLAN_FILE), plan_json().to_string()).unwrap();
        let team = claude.join("teams/session-abcdef12");
        fs::create_dir_all(&team).unwrap();
        fs::write(team.join("config.json"), json!({"members": [{"name": "dev"}]}).to_string()).unwrap();
        let tasks = claude.join("tasks/session-abcdef12");
        fs::create_dir_all(&tasks).unwrap();
        fs::write(tasks.join("1.json"), json!({"id": 1, "subject": "Requisitos", "status": "completed", "owner": "analista"}).to_string()).unwrap();
        fs::write(tasks.join("2.json"), json!({"tasks": [{"taskId": "2", "title": "Implementar\nmás", "state": "in_progress", "assignee": "dev", "blockedBy": [1]}]}).to_string()).unwrap();

        let lead = Agent { id: "abcdef12-0000".into(), cwd: dir.to_string_lossy().into(), alive: true, ..Agent::default() };
        let view = build(&claude, &dir, &[lead]).unwrap();
        assert_eq!(view.team_name.as_deref(), Some("session-abcdef12"));
        assert!(view.live_tasks && view.lead.is_some());
        assert_eq!(view.tasks[1].subject, "Implementar");
        assert_eq!(view.tasks[1].status, RUNNING);
        assert_eq!(view.phase, "3 · Implementación");
        let dev = view.members.iter().find(|m| m.name == "dev").unwrap();
        assert_eq!((dev.state.as_str(), dev.in_team), ("trabajando", true));
        assert_eq!(view.members[0].state, "terminado");
    }
}

#[cfg(all(test, feature = "native"))]
mod alta_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("za-alta-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn registra_con_nombre_del_plan_y_sin_plan_falla() {
        let root = tmp("reg");
        let (state, proj) = (root.join("state"), root.join("p"));
        fs::create_dir_all(proj.join(".zeroagents")).unwrap();
        assert!(register_existing(&state, &proj, None).is_err());
        fs::write(proj.join(PLAN_FILE), r#"{"plan":{"project":{"name":"Demo"}}}"#).unwrap();
        assert_eq!(register_existing(&state, &proj, Some("  ")).unwrap(), "Demo");
        assert_eq!(register_existing(&state, &proj, Some("Otro")).unwrap(), "Otro");
        assert_eq!(projects(&state).len(), 1);
        assert!(register_existing(&state, &root.join("nada"), None).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn valida_el_formulario() {
        let ok = NewProject {
            prompt: "app".into(),
            dir: "/x".into(),
            models: "haiku, sonnet".into(),
            architect_model: "sonnet".into(),
            architect_effort: "medium".into(),
            permission_mode: "acceptEdits".into(),
        };
        let args = ok.cli_args(Path::new("/x")).unwrap();
        assert!(args.windows(2).any(|w| w == ["--models", "haiku,sonnet"]));
        for bad in [
            NewProject { prompt: " ".into(), ..ok.clone() },
            NewProject { models: "gpt".into(), ..ok.clone() },
            NewProject { models: "".into(), ..ok.clone() },
            NewProject { architect_model: "haiku".into(), ..ok.clone() },
            NewProject { permission_mode: "x".into(), ..ok.clone() },
        ] {
            assert!(bad.cli_args(Path::new("/x")).is_err());
        }
    }
}
