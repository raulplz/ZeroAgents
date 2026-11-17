//! Modelo común a todas las plataformas (port de `model.py`).
//!
//! Cada adaptador traduce sus datos a [`Agent`], así la interfaz no necesita
//! saber de dónde viene cada agente.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Estado del agente, ordenado de más a menos urgente para ti.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Espera que apruebes una herramienta.
    Permission,
    /// Te ha hecho una pregunta o espera que apruebes un plan.
    Question,
    Working,
    /// Terminó y espera tu siguiente mensaje.
    YourTurn,
    #[default]
    Unknown,
    Ended,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::Permission,
        Status::Question,
        Status::Working,
        Status::YourTurn,
        Status::Unknown,
        Status::Ended,
    ];

    pub fn needs_you(self) -> bool {
        matches!(self, Status::Permission | Status::Question)
    }

    /// Posición en el orden de urgencia (0 = lo más urgente).
    pub fn rank(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(usize::MAX)
    }

    /// Texto en español, igual que el valor del enum en Python.
    pub fn label(self) -> &'static str {
        match self {
            Status::Permission => "permiso",
            Status::Question => "pregunta",
            Status::Working => "trabajando",
            Status::YourTurn => "tu turno",
            Status::Unknown => "desconocido",
            Status::Ended => "cerrada",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Estimación a precio de lista que calcula Claude Code.
    pub cost_usd: Option<f64>,
    pub context_pct: Option<f64>,
    pub context_size: Option<u64>,
    /// Entrada nueva + escrita en caché.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Contexto releído de la caché (mucho más barato).
    pub cache_read_tokens: u64,
    pub requests: u64,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub platform: String,
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub status: Status,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    pub fast: bool,
    pub usage: Usage,
    /// Qué necesita de ti.
    pub attention_msg: Option<String>,
    /// Última respuesta del agente.
    pub last_message: Option<String>,
    /// Epoch en segundos.
    pub last_activity: Option<f64>,
    pub subagents: usize,
    pub pid: Option<u32>,
    pub kind: Option<String>,
    pub alive: bool,
    pub transcript: Option<String>,
}

/// Límites de uso de la cuenta (comunes a todas las sesiones).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub five_hour_pct: Option<f64>,
    pub five_hour_resets: Option<f64>,
    pub seven_day_pct: Option<f64>,
    pub seven_day_resets: Option<f64>,
    pub observed_at: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub agents: Vec<Agent>,
    /// Por plataforma.
    pub limits: BTreeMap<String, Limits>,
}

/// Lo que el backend envía a la interfaz cada segundo.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub snapshot: Snapshot,
    /// Hora del backend (epoch en segundos), para calcular «hace».
    pub now: f64,
    /// Carpeta personal, para acortar rutas a `~`.
    pub home: String,
    pub show_ended: bool,
    /// Quién manda las notificaciones: «hooks» (Python) o «app» (esta GUI).
    pub notifier: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_sigue_el_orden_de_urgencia() {
        assert!(Status::Permission.rank() < Status::Question.rank());
        assert!(Status::Working.rank() < Status::YourTurn.rank());
        assert!(Status::Unknown.rank() < Status::Ended.rank());
        assert!(Status::Question.needs_you() && !Status::Working.needs_you());
    }

    #[test]
    fn status_se_serializa_en_snake_case() {
        assert_eq!(serde_json::to_string(&Status::YourTurn).unwrap(), "\"your_turn\"");
    }
}
