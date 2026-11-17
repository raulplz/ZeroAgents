//! Archivos JSON compartidos con la versión Python (ver `docs/datos.md`).
//!
//! Mismas reglas que `store.py`:
//! - bloqueo `flock` sobre `<archivo>.lock` (el mismo que usa Python con `fcntl.flock`);
//! - lectura fresca dentro del bloqueo y conservación de los campos desconocidos
//!   (cada tipo lleva un `#[serde(flatten)] extra`);
//! - escritura atómica: temporal en el mismo directorio + `fsync` + `rename`;
//! - si el archivo no es JSON válido se aparta como `<archivo>.corrupto-<n>` en vez de pisarlo;
//! - al migrar un archivo antiguo se guarda antes `<archivo>.bak`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Bloqueo exclusivo entre procesos; se libera al soltarlo.
#[derive(Debug)]
pub struct Lock(#[allow(dead_code)] File);

fn sibling(path: &Path, f: impl FnOnce(&str) -> String) -> PathBuf {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    path.with_file_name(f(&name))
}

/// Toma el bloqueo de `<path>.lock`, esperando si otro proceso lo tiene.
pub fn lock(path: &Path) -> io::Result<Lock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(sibling(path, |n| format!("{n}.lock")))?;
    file.lock()?;
    Ok(Lock(file))
}

static TMP: AtomicU64 = AtomicU64::new(0);

/// Escribe un temporal del mismo directorio y lo renombra encima.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = sibling(path, |n| format!(".{n}.{}-{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Lee, migra, modifica y reescribe `path` bajo bloqueo. Devuelve el valor guardado.
///
/// `migrate` adapta un archivo antiguo y devuelve `true` si lo tocó (entonces se
/// guarda antes `<archivo>.bak`). Solo debe añadir, nunca quitar datos.
pub fn update<T>(path: &Path, migrate: impl FnOnce(&mut T) -> bool, change: impl FnOnce(&mut T)) -> io::Result<T>
where
    T: Serialize + DeserializeOwned + Default,
{
    let _guard = lock(path)?;
    let mut value = T::default();
    let mut existed = false;
    if path.exists() {
        match fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<T>(&t).ok()) {
            Some(v) => (value, existed) = (v, true),
            None => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                fs::copy(path, sibling(path, |n| format!("{n}.corrupto-{stamp}")))?;
            }
        }
    }
    if migrate(&mut value) && existed {
        fs::copy(path, sibling(path, |n| format!("{n}.bak")))?;
    }
    change(&mut value);
    let mut text = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
    text.push('\n');
    write_atomic(path, &text)?;
    Ok(value)
}
