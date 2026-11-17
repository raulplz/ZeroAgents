//! Lectura del estado (port de `store.py`). La GUI solo lee: los hooks de
//! Python son los que escriben, añadiendo líneas o reemplazando instantáneas.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::de::DeserializeOwned;
use serde_json::Value;

/// Lee un JSON entero. `None` si no existe o no es válido.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Lee un `.jsonl` de forma incremental: cada llamada devuelve solo lo nuevo.
#[derive(Debug)]
pub struct JsonlTail {
    pub path: PathBuf,
    offset: u64,
}

impl JsonlTail {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), offset: 0 }
    }

    pub fn read_new(&mut self) -> Vec<Value> {
        let Ok(meta) = fs::metadata(&self.path) else {
            return Vec::new();
        };
        let size = meta.len();
        if size < self.offset {
            // El archivo se ha truncado o reemplazado.
            self.offset = 0;
        }
        if size == self.offset {
            return Vec::new();
        }
        let mut chunk = Vec::with_capacity((size - self.offset) as usize);
        let read = File::open(&self.path).and_then(|mut fh| {
            fh.seek(SeekFrom::Start(self.offset))?;
            fh.take(size - self.offset).read_to_end(&mut chunk)
        });
        if read.is_err() {
            return Vec::new();
        }
        // Deja para la próxima vez una línea que aún se esté escribiendo.
        let end = chunk.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        self.offset += end as u64;
        chunk[..end]
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect()
    }
}

/// Borra estado de ZeroAgents (nunca datos de Claude) con más de N días.
/// Es lo mismo que hace el panel Python al arrancar.
pub fn prune(directory: &Path, max_age_days: f64) {
    let Some(limit) =
        SystemTime::now().checked_sub(Duration::from_secs_f64(max_age_days * 86400.0))
    else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_file() && meta.modified().is_ok_and(|m| m < limit) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Epoch en segundos de la última modificación.
pub fn mtime(path: &Path) -> Option<f64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs_f64())
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
pub(crate) use tests::tempdir;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tail_devuelve_solo_lo_nuevo_y_espera_lineas_a_medias() {
        let dir = tempdir("tail");
        let path = dir.join("x.jsonl");
        let mut fh = File::create(&path).unwrap();
        writeln!(fh, r#"{{"a":1}}"#).unwrap();
        write!(fh, r#"{{"a":2"#).unwrap(); // línea a medio escribir
        fh.flush().unwrap();

        let mut tail = JsonlTail::new(&path);
        assert_eq!(tail.read_new(), vec![serde_json::json!({"a": 1})]);
        assert!(tail.read_new().is_empty());

        writeln!(fh, "}}").unwrap();
        writeln!(fh, "no es json").unwrap();
        fh.flush().unwrap();
        assert_eq!(tail.read_new(), vec![serde_json::json!({"a": 2})]);

        // Si el archivo se reemplaza por uno más corto, vuelve a empezar.
        fs::write(&path, "{\"b\":3}\n").unwrap();
        assert_eq!(tail.read_new(), vec![serde_json::json!({"b": 3})]);
    }

    pub(crate) fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zeroagents-test-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

