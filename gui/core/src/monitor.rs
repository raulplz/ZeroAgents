//! Junta los agentes de todas las plataformas en una sola vista (port de `monitor.py`).

use crate::claude_code::{ClaudeCode, PLATFORM};
use crate::model::Snapshot;

#[derive(Default)]
pub struct Monitor {
    pub claude: ClaudeCode,
}

impl Monitor {
    pub fn new() -> Self {
        Self { claude: ClaudeCode::new() }
    }

    pub fn snapshot(&mut self, include_ended: bool) -> Snapshot {
        let mut snap = Snapshot::default();
        let (agents, limits) = self.claude.collect(include_ended);
        snap.agents = agents;
        if let Some(limits) = limits {
            snap.limits.insert(PLATFORM.into(), limits);
        }
        sort_agents(&mut snap.agents);
        snap
    }
}

/// Primero lo que te necesita; dentro de cada estado, lo más reciente.
pub fn sort_agents(agents: &mut [crate::model::Agent]) {
    agents.sort_by(|a, b| {
        a.status
            .rank()
            .cmp(&b.status.rank())
            .then(b.last_activity.unwrap_or(0.0).total_cmp(&a.last_activity.unwrap_or(0.0)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, Status};

    #[test]
    fn ordena_por_urgencia_y_luego_por_actividad() {
        let a = |id: &str, status, t| Agent { id: id.into(), status, last_activity: Some(t), ..Agent::default() };
        let mut list = vec![
            a("viejo", Status::Working, 1.0),
            a("turno", Status::YourTurn, 9.0),
            a("nuevo", Status::Working, 5.0),
            a("permiso", Status::Permission, 0.0),
        ];
        sort_agents(&mut list);
        let ids: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["permiso", "nuevo", "viejo", "turno"]);
    }
}
