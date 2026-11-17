//! Adaptador de Claude Code (port de `adapters/claude_code.py`).
//!
//! Fuentes, de más a menos fiable:
//! - Hooks (documentados): modo de permisos, esfuerzo y, sobre todo, si el
//!   agente espera un permiso o una respuesta tuya. El hook de Python apunta
//!   cada evento en `<sesión>.events.jsonl`.
//! - Statusline (documentada): modelo, esfuerzo, coste, contexto y límites de
//!   la cuenta, en `<sesión>.status.json`.
//! - `~/.claude/sessions/<pid>.json` (interno, puede cambiar): qué sesiones
//!   están vivas, su nombre y si están ocupadas o libres.
//! - Transcript `.jsonl` (interno): tokens acumulados y respaldo de modelo,
//!   modo y esfuerzo para sesiones abiertas antes de instalar ZeroAgents.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::model::{Agent, Limits, Status, Usage};
use crate::paths;
use crate::store::{self, JsonlTail};

pub const PLATFORM: &str = "claude-code";

const QUESTION_NOTIFICATIONS: [&str; 3] =
    ["elicitation_dialog", "elicitation_url_dialog", "agent_needs_input"];

/// Si el registro dice «libre» bastante después de una petición de permiso,
/// la petición se respondió o se canceló (Esc no dispara ningún hook).
pub const REGISTRY_MARGIN_S: f64 = 2.0;
pub const ENDED_WINDOW_S: f64 = 24.0 * 3600.0;

fn is_question_tool(tool: Option<&str>) -> bool {
    matches!(tool, Some("AskUserQuestion" | "ExitPlanMode"))
}

// ── Eventos de hook (los escribe `zeroagents hook claude-code`) ─────────────

/// Un evento reducido tal y como lo guarda `event_from_hook` en Python.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct HookEvent {
    pub t: f64,
    pub e: String,
    /// Modo de permisos.
    pub pm: Option<String>,
    /// Esfuerzo.
    pub eff: Option<String>,
    /// Id del subagente, si el evento es suyo.
    pub ag: Option<String>,
    pub model: Option<String>,
    /// Título de la sesión.
    pub st: Option<String>,
    pub cwd: Option<String>,
    /// Ruta del transcript.
    pub tp: Option<String>,
    /// Tipo de notificación.
    pub nt: Option<String>,
    pub msg: Option<String>,
    pub tool: Option<String>,
    /// Última respuesta (evento Stop).
    pub last: Option<String>,
}

impl HookEvent {
    /// Atajo para las pruebas: `HookEvent::new("Stop", 4.0)`.
    pub fn new(e: &str, t: f64) -> Self {
        Self { e: e.into(), t, ..Self::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Permission,
    Question,
}

/// Valores con fecha: gana el más reciente (como `remember` en Python).
#[derive(Debug, Clone, Default)]
pub struct Latest(HashMap<&'static str, (String, f64)>);

impl Latest {
    pub fn remember(&mut self, key: &'static str, value: Option<&str>, t: f64) {
        let Some(value) = value.filter(|v| !v.is_empty()) else {
            return;
        };
        if self.0.get(key).is_none_or(|(_, old)| t >= *old) {
            self.0.insert(key, (value.to_string(), t));
        }
    }

    pub fn get(&self, key: &str) -> Option<(&str, f64)> {
        self.0.get(key).map(|(v, t)| (v.as_str(), *t))
    }
}

/// Estado de una sesión plegando sus eventos de hook en orden.
#[derive(Debug, Clone, Default)]
pub struct HookState {
    /// clave (subagente o «main»/«notif») → (tipo, mensaje, t)
    pub pending: HashMap<String, (Need, Option<String>, f64)>,
    pub last_main: Option<String>,
    pub last_main_t: f64,
    pub last_t: f64,
    pub idle_notified: bool,
    pub subagents: HashSet<String>,
    pub values: Latest,
    pub last_message: Option<String>,
}

impl HookState {
    pub fn apply(&mut self, ev: &HookEvent) {
        let (e, t) = (ev.e.as_str(), ev.t);
        let key = ev.ag.clone().unwrap_or_else(|| "main".into());
        self.last_t = self.last_t.max(t);
        for (dst, src) in [
            ("mode", &ev.pm),
            ("effort", &ev.eff),
            ("model", &ev.model),
            ("title", &ev.st),
            ("cwd", &ev.cwd),
            ("tp", &ev.tp),
        ] {
            self.values.remember(dst, src.as_deref(), t);
        }

        if e == "Notification" {
            let nt = ev.nt.as_deref().unwrap_or("");
            if nt == "permission_prompt" {
                if !self.pending.values().any(|p| p.0 == Need::Permission) {
                    self.pending.insert("notif".into(), (Need::Permission, ev.msg.clone(), t));
                }
            } else if QUESTION_NOTIFICATIONS.contains(&nt) {
                self.pending.insert("notif".into(), (Need::Question, ev.msg.clone(), t));
            } else if matches!(nt, "elicitation_complete" | "elicitation_response") {
                self.pending.remove("notif");
            } else if nt == "idle_prompt" {
                self.idle_notified = true;
            }
            return;
        }

        // Cualquier otro evento significa que algo avanzó.
        self.pending.remove("notif");
        self.pending.remove(&key);
        match e {
            "PermissionRequest" => {
                self.pending.insert(key.clone(), (Need::Permission, ev.msg.clone(), t));
            }
            "PreToolUse" if is_question_tool(ev.tool.as_deref()) => {
                self.pending.insert(key.clone(), (Need::Question, ev.msg.clone(), t));
            }
            "SubagentStart" => {
                if let Some(ag) = &ev.ag {
                    self.subagents.insert(ag.clone());
                }
            }
            "SubagentStop" => {
                if let Some(ag) = &ev.ag {
                    self.subagents.remove(ag);
                }
            }
            "UserPromptSubmit" | "Stop" | "SessionStart" | "SessionEnd" if key == "main" => {
                self.pending.clear();
                if matches!(e, "SessionStart" | "SessionEnd") {
                    self.subagents.clear();
                }
            }
            _ => {}
        }

        if key == "main" && !matches!(e, "SubagentStart" | "SubagentStop") {
            self.last_main = Some(e.to_string());
            self.last_main_t = t;
            self.idle_notified = false;
            if e == "Stop" && ev.last.is_some() {
                self.last_message = ev.last.clone();
            }
        }
    }

    /// Estado según los hooks: (estado, qué necesita, desde cuándo).
    pub fn status(&self) -> (Status, Option<String>, f64) {
        for (need, status) in [(Need::Permission, Status::Permission), (Need::Question, Status::Question)] {
            let newest = self
                .pending
                .values()
                .filter(|p| p.0 == need)
                .fold(None::<&(Need, Option<String>, f64)>, |best, p| match best {
                    Some(b) if b.2 >= p.2 => Some(b),
                    _ => Some(p),
                });
            if let Some((_, msg, t)) = newest {
                return (status, msg.clone(), *t);
            }
        }
        match self.last_main.as_deref() {
            Some("SessionEnd") => (Status::Ended, None, self.last_main_t),
            Some("Stop" | "SessionStart") => (Status::YourTurn, None, self.last_main_t),
            _ if self.idle_notified => (Status::YourTurn, None, self.last_main_t),
            Some(_) => (Status::Working, None, self.last_main_t),
            None => (Status::Unknown, None, 0.0),
        }
    }
}

/// Aplica los eventos ordenados por fecha (orden estable, como `sorted`).
pub fn fold(mut events: Vec<HookEvent>, state: &mut HookState) {
    events.sort_by(|a, b| a.t.total_cmp(&b.t));
    for ev in &events {
        state.apply(ev);
    }
}

// ── Transcript ──────────────────────────────────────────────────────────────

/// Fecha ISO 8601 → epoch en segundos (0 si falta o no se entiende).
pub fn iso(ts: Option<&str>) -> f64 {
    let Some(ts) = ts.filter(|s| !s.is_empty()) else {
        return 0.0;
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt.timestamp_millis() as f64 / 1000.0;
    }
    // Sin zona horaria: Python la trataría como hora local; aquí, como UTC.
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f")
        .map_or(0.0, |dt| dt.and_utc().timestamp_millis() as f64 / 1000.0)
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn u64_at(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Tokens acumulados y valores de respaldo sacados del transcript.
#[derive(Debug, Clone, Default)]
pub struct TranscriptStats {
    pub values: Latest,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub requests: u64,
    seen: HashSet<String>,
    pub last_t: f64,
}

impl TranscriptStats {
    pub fn apply(&mut self, d: &Value) {
        let t = iso(str_at(d, "timestamp"));
        self.last_t = self.last_t.max(t);
        match str_at(d, "type") {
            Some("assistant") => {
                let msg = d.get("message").unwrap_or(&Value::Null);
                if let Some(model) = str_at(msg, "model").filter(|m| !m.starts_with('<')) {
                    self.values.remember("model", Some(model), t);
                }
                self.values.remember("effort", str_at(d, "effort"), t);
                let rid = str_at(d, "requestId").or_else(|| str_at(msg, "id")).unwrap_or("");
                // Una respuesta puede ocupar varias líneas con el mismo uso.
                if let Some(usage) = msg.get("usage").filter(|u| u.is_object())
                    && self.seen.insert(rid.to_string())
                {
                    self.requests += 1;
                    self.input_tokens +=
                        u64_at(usage, "input_tokens") + u64_at(usage, "cache_creation_input_tokens");
                    self.cache_read_tokens += u64_at(usage, "cache_read_input_tokens");
                    self.output_tokens += u64_at(usage, "output_tokens");
                }
            }
            Some("user") => self.values.remember("mode", str_at(d, "permissionMode"), t),
            Some("ai-title") => {
                let when = if t > 0.0 { t } else { self.last_t };
                self.values.remember("title", str_at(d, "aiTitle"), when);
            }
            Some("custom-title") => {
                let when = if t > 0.0 { t } else { self.last_t };
                self.values.remember("custom_title", str_at(d, "customTitle"), when);
            }
            _ => {}
        }
    }
}

/// `/home/ana/x` → `~/.claude/projects/-home-ana-x/<sesión>.jsonl`.
pub fn transcript_path_for(claude: &Path, cwd: &str, sid: &str) -> PathBuf {
    let slug: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let projects = claude.join("projects");
    let guess = projects.join(slug).join(format!("{sid}.jsonl"));
    if guess.exists() {
        return guess;
    }
    fs::read_dir(&projects)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join(format!("{sid}.jsonl")))
        .find(|p| p.exists())
        .unwrap_or(guess)
}

// ── Registro de sesiones vivas ──────────────────────────────────────────────

/// Una entrada de `~/.claude/sessions/<pid>.json` (formato interno).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RegistryEntry {
    pub session_id: Option<String>,
    pub pid: Option<u32>,
    /// Número o texto según la versión: se compara como texto.
    pub proc_start: Option<Value>,
    pub cwd: Option<String>,
    pub status: Option<String>,
    pub status_updated_at: Option<f64>,
    pub updated_at: Option<f64>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub kind: Option<String>,
}

pub fn read_registry(claude: &Path) -> HashMap<String, RegistryEntry> {
    let mut out = HashMap::new();
    for entry in fs::read_dir(claude.join("sessions")).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && let Some(reg) = store::read_json::<RegistryEntry>(&path)
            && let Some(sid) = reg.session_id.clone()
        {
            out.insert(sid, reg);
        }
    }
    out
}

fn value_as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// ¿Sigue vivo el proceso? En Linux compara además su hora de arranque
/// (campo 22 de `/proc/<pid>/stat`) para no confundir un PID reutilizado.
pub fn pid_alive(pid: Option<u32>, proc_start: Option<&Value>) -> bool {
    let Some(pid) = pid.filter(|p| *p > 0) else {
        return false;
    };
    if cfg!(target_os = "linux") {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        if let Some(start) = proc_start.and_then(value_as_text) {
            let after = stat.rsplit_once(')').map_or("", |(_, rest)| rest);
            return after.split_whitespace().nth(19) == Some(start.as_str());
        }
        return true;
    }
    true
}

// ── Adaptador ───────────────────────────────────────────────────────────────

/// Instantánea de la statusline (`<sesión>.status.json`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StatusSnap {
    pub t: f64,
    pub model_id: Option<String>,
    pub effort: Option<String>,
    pub fast: bool,
    pub cost: Option<f64>,
    pub duration_ms: Option<f64>,
    pub ctx_pct: Option<f64>,
    pub ctx_size: Option<f64>,
    pub session_name: Option<String>,
    pub cwd: Option<String>,
    pub tp: Option<String>,
    pub rate_limits: Option<RateLimits>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RateLimits {
    pub five_hour: Option<RateWindow>,
    pub seven_day: Option<RateWindow>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RateWindow {
    pub used_percentage: Option<f64>,
    pub resets_at: Option<f64>,
}

pub fn limits_from(snap: &StatusSnap) -> Option<Limits> {
    let rate = snap.rate_limits.as_ref()?;
    let five = rate.five_hour.clone().unwrap_or_default();
    let seven = rate.seven_day.clone().unwrap_or_default();
    Some(Limits {
        five_hour_pct: five.used_percentage,
        five_hour_resets: five.resets_at,
        seven_day_pct: seven.used_percentage,
        seven_day_resets: seven.resets_at,
        observed_at: Some(snap.t),
    })
}

/// De varias opciones (valor, fecha), la más reciente; en empate, la primera.
fn newest<'a>(options: impl IntoIterator<Item = Option<(&'a str, f64)>>) -> Option<String> {
    options
        .into_iter()
        .flatten()
        .filter(|(v, _)| !v.is_empty())
        .fold(None::<(&str, f64)>, |best, o| match best {
            Some(b) if b.1 >= o.1 => Some(b),
            _ => Some(o),
        })
        .map(|(v, _)| v.to_string())
}

pub struct ClaudeCode {
    /// Estado de ZeroAgents para Claude Code.
    pub dir: PathBuf,
    /// Configuración de Claude Code (`~/.claude`).
    pub claude: PathBuf,
    events: HashMap<String, (JsonlTail, HookState)>,
    transcripts: HashMap<PathBuf, (JsonlTail, TranscriptStats)>,
}

impl Default for ClaudeCode {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCode {
    pub fn new() -> Self {
        Self::with_dirs(paths::platform_dir(PLATFORM), paths::claude_dir())
    }

    /// Para pruebas: carpetas propias en vez de las reales.
    pub fn with_dirs(dir: PathBuf, claude: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        store::prune(&dir, 7.0);
        Self { dir, claude, events: HashMap::new(), transcripts: HashMap::new() }
    }

    fn hook_state(&mut self, sid: &str) -> &HookState {
        let dir = &self.dir;
        let (tail, state) = self.events.entry(sid.to_string()).or_insert_with(|| {
            (JsonlTail::new(dir.join(format!("{sid}.events.jsonl"))), HookState::default())
        });
        let new: Vec<HookEvent> =
            tail.read_new().into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
        fold(new, state);
        state
    }

    fn transcript(&mut self, path: &Path) -> &TranscriptStats {
        let (tail, stats) = self
            .transcripts
            .entry(path.to_path_buf())
            .or_insert_with(|| (JsonlTail::new(path), TranscriptStats::default()));
        for d in tail.read_new() {
            stats.apply(&d);
        }
        stats
    }

    /// Sesiones con estado propio tocado en las últimas 24 h.
    fn known_sessions(&self) -> HashSet<String> {
        let limit = store::now() - ENDED_WINDOW_S;
        fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.contains(".json") {
                    return None;
                }
                let fresh = store::mtime(&entry.path()).is_some_and(|m| m >= limit);
                fresh.then(|| name.split('.').next().unwrap_or("").to_string())
            })
            .filter(|sid| !sid.is_empty())
            .collect()
    }

    pub fn collect(&mut self, include_ended: bool) -> (Vec<Agent>, Option<Limits>) {
        let live: HashMap<String, RegistryEntry> = read_registry(&self.claude)
            .into_iter()
            .filter(|(_, e)| pid_alive(e.pid, e.proc_start.as_ref()))
            .collect();
        let mut sids: Vec<String> = live.keys().cloned().collect();
        if include_ended {
            for sid in self.known_sessions() {
                if !live.contains_key(&sid) {
                    sids.push(sid);
                }
            }
        }
        let mut agents = Vec::new();
        let mut limits: Option<Limits> = None;
        for sid in sids {
            let (agent, rate) = self.build(&sid, live.get(&sid));
            if agent.alive || include_ended {
                agents.push(agent);
            }
            if let Some(rate) = rate
                && limits.as_ref().is_none_or(|l| {
                    rate.observed_at.unwrap_or(0.0) > l.observed_at.unwrap_or(0.0)
                })
            {
                limits = Some(rate);
            }
        }
        (agents, limits)
    }

    pub fn build(&mut self, sid: &str, reg: Option<&RegistryEntry>) -> (Agent, Option<Limits>) {
        let hooks = self.hook_state(sid).clone();
        let snap: StatusSnap =
            store::read_json(&self.dir.join(format!("{sid}.status.json"))).unwrap_or_default();
        let snap_t = snap.t;
        let cwd = reg
            .and_then(|r| r.cwd.clone())
            .filter(|c| !c.is_empty())
            .or_else(|| snap.cwd.clone().filter(|c| !c.is_empty()))
            .or_else(|| hooks.values.get("cwd").map(|(v, _)| v.to_string()))
            .unwrap_or_default();
        let tp = snap
            .tp
            .clone()
            .filter(|p| !p.is_empty())
            .or_else(|| hooks.values.get("tp").map(|(v, _)| v.to_string()));
        let tpath = match tp {
            Some(p) => PathBuf::from(p),
            None => transcript_path_for(&self.claude, &cwd, sid),
        };
        let tx = self.transcript(&tpath).clone();

        let latest = |key: &str, extra: Option<(&str, f64)>| {
            newest([hooks.values.get(key), tx.values.get(key), extra])
        };
        let model = latest("model", snap.model_id.as_deref().map(|m| (m, snap_t)));
        let effort = latest("effort", snap.effort.as_deref().map(|e| (e, snap_t)));
        let mode = latest("mode", None);

        let (mut status, mut need, since) = hooks.status();
        let alive = reg.is_some();
        if let Some(reg) = reg {
            let reg_t = reg.status_updated_at.unwrap_or(0.0) / 1000.0;
            let reg_status = match reg.status.as_deref() {
                Some("busy") => Some(Status::Working),
                Some("idle") => Some(Status::YourTurn),
                _ => None,
            };
            if status.needs_you() {
                if reg_status == Some(Status::YourTurn) && reg_t > since + REGISTRY_MARGIN_S {
                    status = Status::YourTurn;
                    need = None;
                }
            } else if let Some(rs) = reg_status
                && (reg_t > hooks.last_t || matches!(status, Status::Unknown | Status::Ended))
            {
                status = rs;
            }
        } else {
            status = Status::Ended;
            need = None;
        }

        let name = if reg.is_some_and(|r| r.name_source.as_deref() == Some("user")) {
            reg.and_then(|r| r.name.clone())
        } else {
            snap.session_name
                .clone()
                .filter(|n| !n.is_empty())
                .or_else(|| latest("custom_title", None))
                .or_else(|| latest("title", None))
                .or_else(|| reg.and_then(|r| r.name.clone()))
        };
        let usage = Usage {
            cost_usd: snap.cost,
            context_pct: snap.ctx_pct,
            context_size: snap.ctx_size.map(|v| v as u64),
            input_tokens: tx.input_tokens,
            output_tokens: tx.output_tokens,
            cache_read_tokens: tx.cache_read_tokens,
            requests: tx.requests,
            duration_ms: snap.duration_ms.map(|v| v as u64),
        };
        let updated = reg.and_then(|r| r.updated_at).unwrap_or(0.0) / 1000.0;
        let last_activity = [hooks.last_t, tx.last_t, snap_t, updated].into_iter().fold(0.0, f64::max);
        let agent = Agent {
            platform: PLATFORM.into(),
            id: sid.into(),
            name: name.filter(|n| !n.is_empty()).unwrap_or_else(|| sid.chars().take(8).collect()),
            cwd,
            status,
            model,
            effort,
            mode,
            fast: snap.fast,
            usage,
            attention_msg: need,
            last_message: hooks.last_message.clone(),
            last_activity: (last_activity > 0.0).then_some(last_activity),
            subagents: hooks.subagents.len(),
            pid: reg.and_then(|r| r.pid),
            kind: reg.and_then(|r| r.kind.clone()),
            alive,
            transcript: Some(tpath.to_string_lossy().into()),
        };
        (agent, limits_from(&snap))
    }
}

#[cfg(test)]
mod tests {
    //! Las mismas pruebas que `tests/test_claude_code.py`, más algunas propias.
    use super::*;
    use crate::store::tempdir;
    use serde_json::json;

    fn ev(e: &str, t: f64) -> HookEvent {
        HookEvent::new(e, t)
    }

    fn folded(events: Vec<HookEvent>) -> HookState {
        let mut state = HookState::default();
        fold(events, &mut state);
        state
    }

    #[test]
    fn permission_request_needs_you_until_tool_runs() {
        let mut state = folded(vec![
            HookEvent { pm: Some("default".into()), ..ev("UserPromptSubmit", 1.0) },
            HookEvent { tool: Some("Bash".into()), msg: Some("Bash: borrar build".into()), ..ev("PermissionRequest", 2.0) },
        ]);
        let (status, need, _) = state.status();
        assert_eq!(status, Status::Permission);
        assert_eq!(need.as_deref(), Some("Bash: borrar build"));
        fold(vec![HookEvent { tool: Some("Bash".into()), ..ev("PostToolUse", 3.0) }], &mut state);
        assert_eq!(state.status().0, Status::Working);
    }

    #[test]
    fn subagent_permission_not_cleared_by_main_agent_activity() {
        let mut state = folded(vec![
            HookEvent { ag: Some("a1".into()), ..ev("SubagentStart", 1.0) },
            HookEvent { ag: Some("a1".into()), msg: Some("Write: x.py".into()), ..ev("PermissionRequest", 2.0) },
            HookEvent { tool: Some("Read".into()), ..ev("PreToolUse", 3.0) },
        ]);
        assert_eq!(state.status().0, Status::Permission);
        assert_eq!(state.subagents.len(), 1);
        fold(
            vec![
                HookEvent { ag: Some("a1".into()), ..ev("PostToolUse", 4.0) },
                HookEvent { ag: Some("a1".into()), ..ev("SubagentStop", 5.0) },
            ],
            &mut state,
        );
        assert_eq!(state.status().0, Status::Working);
        assert!(state.subagents.is_empty());
    }

    #[test]
    fn question_tools_and_stop() {
        let mut state = folded(vec![
            ev("UserPromptSubmit", 1.0),
            HookEvent {
                tool: Some("AskUserQuestion".into()),
                msg: Some("¿Qué base de datos?".into()),
                ..ev("PreToolUse", 2.0)
            },
        ]);
        let (status, need, _) = state.status();
        assert_eq!((status, need.as_deref()), (Status::Question, Some("¿Qué base de datos?")));
        fold(
            vec![
                HookEvent { tool: Some("AskUserQuestion".into()), ..ev("PostToolUse", 3.0) },
                HookEvent { last: Some("Hecho.".into()), ..ev("Stop", 4.0) },
            ],
            &mut state,
        );
        assert_eq!(state.status().0, Status::YourTurn);
        assert_eq!(state.last_message.as_deref(), Some("Hecho."));
    }

    #[test]
    fn permission_notification_and_idle() {
        let notif = |nt: &str, t: f64| HookEvent {
            nt: Some(nt.into()),
            msg: Some("x".into()),
            ..ev("Notification", t)
        };
        let mut state = folded(vec![ev("UserPromptSubmit", 1.0), notif("permission_prompt", 8.0)]);
        assert_eq!(state.status().0, Status::Permission);
        fold(vec![ev("PostToolUse", 9.0)], &mut state);
        assert_eq!(state.status().0, Status::Working);
        fold(vec![ev("Stop", 10.0), notif("idle_prompt", 70.0)], &mut state);
        assert_eq!(state.status().0, Status::YourTurn);
    }

    #[test]
    fn mode_and_effort_keep_latest_value() {
        // Llegan desordenados: gana el de fecha más reciente, no el último leído.
        let state = folded(vec![
            HookEvent { pm: Some("plan".into()), ..ev("UserPromptSubmit", 5.0) },
            HookEvent { pm: Some("default".into()), eff: Some("high".into()), ..ev("PostToolUse", 2.0) },
        ]);
        assert_eq!(state.values.get("mode").map(|v| v.0), Some("plan"));
        assert_eq!(state.values.get("effort").map(|v| v.0), Some("high"));
    }

    #[test]
    fn session_end_closes_and_unknown_without_events() {
        assert_eq!(folded(vec![]).status().0, Status::Unknown);
        assert_eq!(folded(vec![ev("SessionStart", 1.0), ev("SessionEnd", 2.0)]).status().0, Status::Ended);
    }

    #[test]
    fn hook_event_reads_python_json_lines() {
        let line = json!({"t": 1.5, "e": "PermissionRequest", "pm": "default", "eff": "high",
                          "tool": "Bash", "msg": "Bash: Borrar build"});
        let parsed: HookEvent = serde_json::from_value(line).unwrap();
        assert_eq!(parsed.e, "PermissionRequest");
        assert_eq!(parsed.msg.as_deref(), Some("Bash: Borrar build"));
        assert_eq!(parsed.ag, None);
    }

    #[test]
    fn transcript_deduplicates_split_responses() {
        let mut stats = TranscriptStats::default();
        let usage = json!({"input_tokens": 2, "cache_creation_input_tokens": 10,
                           "cache_read_input_tokens": 100, "output_tokens": 5});
        let line = json!({"type": "assistant", "requestId": "r1", "timestamp": "2026-10-26T20:00:00Z",
                          "effort": "high", "message": {"model": "claude-opus-5-5", "usage": usage}});
        stats.apply(&line);
        stats.apply(&line); // misma respuesta en dos líneas
        stats.apply(&json!({"type": "user", "permissionMode": "auto", "timestamp": "2026-10-26T20:01:00Z"}));
        assert_eq!(
            (stats.requests, stats.input_tokens, stats.cache_read_tokens, stats.output_tokens),
            (1, 12, 100, 5)
        );
        assert_eq!(stats.values.get("model").map(|v| v.0), Some("claude-opus-5-5"));
        assert_eq!(stats.values.get("mode").map(|v| v.0), Some("auto"));
        assert_eq!(stats.last_t, iso(Some("2026-10-26T20:01:00Z")));
    }

    #[test]
    fn transcript_ignores_synthetic_models() {
        let mut stats = TranscriptStats::default();
        stats.apply(&json!({"type": "assistant", "message": {"model": "<synthetic>"}}));
        assert!(stats.values.get("model").is_none());
    }

    #[test]
    fn iso_parses_z_offsets_and_garbage() {
        assert_eq!(iso(Some("1970-01-01T00:01:00Z")), 60.0);
        assert_eq!(iso(Some("1970-01-01T00:01:00.500+00:00")), 60.5);
        assert_eq!(iso(Some("ayer")), 0.0);
        assert_eq!(iso(None), 0.0);
    }

    #[test]
    fn registry_idle_clears_stale_permission() {
        // Pulsar Esc en un permiso no dispara hooks: el registro «idle» lo resuelve.
        let root = tempdir("registry");
        let mut adapter = ClaudeCode::with_dirs(root.join("state"), root.join("claude"));
        let line = json!({"e": "PermissionRequest", "t": 100.0, "msg": "Bash: ls"});
        fs::write(adapter.dir.join("s1.events.jsonl"), format!("{line}\n")).unwrap();

        let busy = RegistryEntry {
            session_id: Some("s1".into()),
            status: Some("busy".into()),
            status_updated_at: Some(100_500.0),
            cwd: Some("/x".into()),
            ..Default::default()
        };
        assert_eq!(adapter.build("s1", Some(&busy)).0.status, Status::Permission);
        let idle = RegistryEntry { status: Some("idle".into()), status_updated_at: Some(110_000.0), ..busy };
        let agent = adapter.build("s1", Some(&idle)).0;
        assert_eq!(agent.status, Status::YourTurn);
        assert_eq!(agent.attention_msg, None);
    }

    #[test]
    fn build_merges_statusline_transcript_and_registry() {
        let root = tempdir("build");
        let mut adapter = ClaudeCode::with_dirs(root.join("state"), root.join("claude"));
        let cwd = "/home/ana/proyecto";
        let tdir = adapter.claude.join("projects/-home-ana-proyecto");
        fs::create_dir_all(&tdir).unwrap();
        let tx = [
            json!({"type": "assistant", "requestId": "r1", "timestamp": "2026-11-01T10:00:00Z",
                   "message": {"model": "claude-sonnet-5-5", "usage": {"input_tokens": 3, "output_tokens": 7}}}),
            json!({"type": "ai-title", "aiTitle": "Arreglar el login"}),
        ];
        let body: String = tx.iter().map(|l| format!("{l}\n")).collect();
        fs::write(tdir.join("s2.jsonl"), body).unwrap();
        let snap = json!({"t": 1.0, "model_id": "claude-opus-5-5", "cost": 0.42, "ctx_pct": 37,
                          "ctx_size": 200000, "rate_limits": {"five_hour": {"used_percentage": 92, "resets_at": 1794928800},
                                                              "seven_day": {"used_percentage": 16}}});
        fs::write(adapter.dir.join("s2.status.json"), snap.to_string()).unwrap();

        let reg = RegistryEntry {
            session_id: Some("s2".into()),
            cwd: Some(cwd.into()),
            status: Some("busy".into()),
            status_updated_at: Some(5_000.0),
            pid: Some(4242),
            name: Some("derivado".into()),
            name_source: Some("derived".into()),
            ..Default::default()
        };
        let (agent, limits) = adapter.build("s2", Some(&reg));
        // El transcript es más reciente que la statusline (t=1): gana su modelo.
        assert_eq!(agent.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(agent.name, "Arreglar el login");
        assert_eq!(agent.status, Status::Working);
        assert_eq!((agent.usage.input_tokens, agent.usage.output_tokens), (3, 7));
        assert_eq!(agent.usage.cost_usd, Some(0.42));
        assert_eq!(agent.usage.context_size, Some(200_000));
        let limits = limits.unwrap();
        assert_eq!((limits.five_hour_pct, limits.seven_day_pct), (Some(92.0), Some(16.0)));
        assert_eq!(limits.seven_day_resets, None);

        // Un nombre puesto por el usuario manda sobre el título automático.
        let named = RegistryEntry { name: Some("Mi sesión".into()), name_source: Some("user".into()), ..reg };
        assert_eq!(adapter.build("s2", Some(&named)).0.name, "Mi sesión");
        // Sin registro, la sesión está cerrada.
        assert_eq!(adapter.build("s2", None).0.status, Status::Ended);
    }

    #[test]
    fn transcript_path_guess_and_fallback() {
        let root = tempdir("tpath");
        let claude = root.join("claude");
        let other = claude.join("projects/-otra-carpeta");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("s9.jsonl"), "").unwrap();
        // No está donde diría la carpeta, pero se encuentra buscando.
        assert_eq!(transcript_path_for(&claude, "/x/y", "s9"), other.join("s9.jsonl"));
        assert_eq!(
            transcript_path_for(&claude, "/x/y", "nada"),
            claude.join("projects/-x-y/nada.jsonl")
        );
    }

    #[test]
    fn pid_alive_checks_start_time() {
        let me = std::process::id();
        assert!(pid_alive(Some(me), None));
        assert!(!pid_alive(Some(me), Some(&json!("0"))));
        assert!(!pid_alive(None, None));
    }
}
