//! Frontend de ZeroAgents: Leptos en modo CSR, compilado a wasm con trunk.
//!
//! Comparte tipos y formato con el backend a través de `zeroagents-core`
//! (sin la feature `native`).

mod app;
mod mensajes;
mod sesiones;
mod tauri;
mod util;
mod work;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
