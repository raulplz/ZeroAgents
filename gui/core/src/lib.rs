//! Núcleo de ZeroAgents en Rust.
//!
//! Es un port fiel del paquete Python (`src/zeroagents`): lee las mismas
//! fuentes y llega a las mismas conclusiones sobre cada sesión.
//!
//! - [`model`] y [`fmt`] son puros y compilan también a wasm, así el frontend
//!   Leptos comparte los tipos y el formato con el backend.
//! - Lo demás necesita la feature `native` (archivos, procesos, Hyprland).
//!
//! La ingesta (hooks y statusline) sigue en Python: Claude Code llama a
//! `zeroagents hook` y `zeroagents statusline`, que escriben el estado que
//! aquí se lee.

pub mod fmt;
pub mod messages;
pub mod model;
pub mod theme;
pub mod work;

#[cfg(feature = "native")]
pub mod claude_code;
#[cfg(feature = "native")]
pub mod jsonfile;
#[cfg(feature = "native")]
pub mod focus;
#[cfg(feature = "native")]
pub mod monitor;
#[cfg(feature = "native")]
pub mod notify;
#[cfg(feature = "native")]
pub mod paths;
#[cfg(feature = "native")]
pub mod store;
