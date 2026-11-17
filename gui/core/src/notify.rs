//! Notificaciones de escritorio (port de `notify.py`).
//!
//! Hoy las manda el hook de Python cuando Claude Code avisa (permiso,
//! pregunta o un minuto esperando). La GUI solo notifica si esos hooks se
//! instalaron con `--no-notify`, para no duplicar avisos.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::model::{Agent, Status};

/// Igual que `notify.send` en Python. Si no hay forma de notificar, no hace nada.
pub fn send(title: &str, body: &str, urgent: bool) {
    let mut cmd = Command::new("notify-send");
    cmd.args(["-a", "ZeroAgents", "-i", "dialog-information"]);
    if urgent {
        cmd.args(["-u", "critical"]);
    }
    let _ = cmd.args([title, body]).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// ¿Los hooks de ZeroAgents ya notifican? Lee `settings.json` sin tocarlo.
pub fn hooks_notify(claude: &Path) -> bool {
    let Some(settings) = crate::store::read_json::<Value>(&claude.join("settings.json")) else {
        return false;
    };
    let mut commands = Vec::new();
    collect_commands(settings.get("hooks").unwrap_or(&Value::Null), &mut commands);
    commands
        .iter()
        .any(|c| c.contains("zeroagents") && c.contains(" hook claude-code") && !c.contains("--no-notify"))
}

fn collect_commands(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                match (k.as_str(), v) {
                    ("command", Value::String(s)) => out.push(s.clone()),
                    _ => collect_commands(v, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_commands(i, out)),
        _ => {}
    }
}

/// Un aviso pendiente de mandar.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub title: String,
    pub body: String,
    pub urgent: bool,
}

/// Imita los avisos de Claude Code: tras unos segundos esperando un permiso o
/// una respuesta, y tras un minuto esperando tu siguiente mensaje. Cada
/// espera se avisa una sola vez.
#[derive(Debug, Default)]
pub struct Notifier {
    /// sesión → (clave de la espera, desde cuándo, ya avisada)
    waiting: HashMap<String, (String, f64, bool)>,
}

pub const NEED_DELAY_S: f64 = 6.0;
pub const IDLE_DELAY_S: f64 = 60.0;

impl Notifier {
    pub fn check(&mut self, agents: &[Agent], now: f64) -> Vec<Alert> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for a in agents.iter().filter(|a| a.alive) {
            let key = match a.status {
                Status::Permission | Status::Question => {
                    format!("{:?}:{}", a.status, a.attention_msg.as_deref().unwrap_or(""))
                }
                Status::YourTurn => format!("turno:{}", a.last_activity.unwrap_or(0.0)),
                _ => continue,
            };
            seen.push(a.id.clone());
            let entry = self.waiting.entry(a.id.clone()).or_insert((key.clone(), now, false));
            if entry.0 != key {
                *entry = (key, now, false);
            }
            let delay = if a.status.needs_you() { NEED_DELAY_S } else { IDLE_DELAY_S };
            if !entry.2 && now - entry.1 >= delay {
                entry.2 = true;
                let body = match a.status {
                    Status::YourTurn => "Espera tu siguiente mensaje".to_string(),
                    _ => a.attention_msg.clone().unwrap_or_else(|| "Te necesita".into()),
                };
                out.push(Alert { title: format!("ZeroAgents · {}", a.name), body, urgent: false });
            }
        }
        self.waiting.retain(|id, _| seen.contains(id));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tempdir;

    fn agent(status: Status, msg: Option<&str>) -> Agent {
        Agent {
            id: "s1".into(),
            name: "web".into(),
            alive: true,
            status,
            attention_msg: msg.map(Into::into),
            ..Agent::default()
        }
    }

    #[test]
    fn avisa_una_vez_tras_la_espera() {
        let mut n = Notifier::default();
        let list = [agent(Status::Permission, Some("Bash: ls"))];
        assert!(n.check(&list, 100.0).is_empty());
        let alerts = n.check(&list, 100.0 + NEED_DELAY_S);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].body, "Bash: ls");
        assert!(n.check(&list, 200.0).is_empty(), "no repite");
        // Una petición nueva vuelve a contar.
        let other = [agent(Status::Permission, Some("Write: a.py"))];
        assert!(n.check(&other, 201.0).is_empty());
        assert_eq!(n.check(&other, 210.0).len(), 1);
    }

    #[test]
    fn tu_turno_espera_un_minuto() {
        let mut n = Notifier::default();
        let list = [agent(Status::YourTurn, None)];
        assert!(n.check(&list, 0.0).is_empty());
        assert!(n.check(&list, 30.0).is_empty());
        assert_eq!(n.check(&list, 61.0)[0].body, "Espera tu siguiente mensaje");
    }

    #[test]
    fn detecta_hooks_que_ya_notifican() {
        let dir = tempdir("hooks");
        let hook = |cmd: &str| {
            serde_json::json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": cmd}]}]}})
        };
        std::fs::write(dir.join("settings.json"), hook("/x/zeroagents hook claude-code").to_string()).unwrap();
        assert!(hooks_notify(&dir));
        std::fs::write(dir.join("settings.json"), hook("/x/zeroagents hook claude-code --no-notify").to_string())
            .unwrap();
        assert!(!hooks_notify(&dir));
    }
}
