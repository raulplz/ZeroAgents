//! Rutas de estado (port de `paths.py`). Respeta las mismas variables de
//! entorno que la versión Python para que ambas lean el mismo estado.

use std::env;
use std::path::PathBuf;

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// `~/.local/state/zeroagents`, o `ZEROAGENTS_STATE_DIR` si está definida.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = env::var_os("ZEROAGENTS_STATE_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if cfg!(target_os = "macos") {
        return home().join("Library/Application Support/ZeroAgents");
    }
    let base = env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"));
    base.join("zeroagents")
}

/// Carpeta de estado de una plataforma. La crea si no existe, como en Python.
pub fn platform_dir(platform: &str) -> PathBuf {
    let path = state_dir().join(platform);
    let _ = std::fs::create_dir_all(&path);
    path
}

/// `~/.claude`, o `CLAUDE_CONFIG_DIR` si está definida.
pub fn claude_dir() -> PathBuf {
    env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}

/// `~/.config` (o `XDG_CONFIG_HOME`).
pub fn config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}
