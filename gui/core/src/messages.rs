//! Mensajes entre agentes (lectura; port de la parte de `messages.py` que lee).
//!
//! Los captura el hook de Python en `<sesión>.messages.jsonl` y, si el
//! proyecto tiene `.zeroagents/`, en `.zeroagents/mensajes.jsonl`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Message {
    pub t: f64,
    /// «encargo», «entrega» o «mensaje».
    pub kind: String,
    pub sesion: Option<String>,
    pub de: Option<String>,
    pub para: Option<String>,
    pub titulo: Option<String>,
    pub texto: String,
    pub via: Option<String>,
}

impl Message {
    pub fn label(&self) -> &str {
        match self.kind.as_str() {
            "encargo" => "ENCARGO",
            "entrega" => "ENTREGA",
            "mensaje" => "MENSAJE",
            _ => "?",
        }
    }
}

#[cfg(feature = "native")]
pub use native::*;

#[cfg(feature = "native")]
mod native {
    use std::path::{Path, PathBuf};

    use super::Message;
    use crate::store::JsonlTail;

    pub const PROJECT_LOG: &str = ".zeroagents/mensajes.jsonl";

    /// Archivos de mensajes del estado propio (todas las sesiones).
    pub fn sources(state: &Path) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(state)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".messages.jsonl"))
            .collect();
        out.sort();
        out
    }

    /// Todos los mensajes desde `since`, en orden de llegada.
    pub fn load(paths: &[PathBuf], since: Option<f64>) -> Vec<Message> {
        let mut out: Vec<Message> = paths
            .iter()
            .flat_map(|p| JsonlTail::new(p).read_new())
            .filter_map(|v| serde_json::from_value(v).ok())
            .filter(|m: &Message| since.is_none_or(|s| m.t >= s))
            .collect();
        out.sort_by(|a, b| a.t.total_cmp(&b.t));
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::store::tempdir;

        #[test]
        fn carga_y_ordena_mensajes_de_varias_sesiones() {
            let dir = tempdir("msgs");
            std::fs::write(
                dir.join("s1.messages.jsonl"),
                "{\"t\":5,\"kind\":\"entrega\",\"de\":\"qa\",\"para\":\"principal\",\"texto\":\"ok\"}\n",
            )
            .unwrap();
            std::fs::write(
                dir.join("s2.messages.jsonl"),
                "{\"t\":1,\"kind\":\"encargo\",\"de\":\"principal\",\"para\":\"qa\",\"titulo\":\"Probar\",\"texto\":\"prueba\"}\n",
            )
            .unwrap();
            std::fs::write(dir.join("s1.events.jsonl"), "{\"t\":0}\n").unwrap();
            let msgs = load(&sources(&dir), None);
            assert_eq!(msgs.iter().map(|m| m.label()).collect::<Vec<_>>(), ["ENCARGO", "ENTREGA"]);
            assert_eq!(load(&sources(&dir), Some(2.0)).len(), 1);
        }
    }
}
