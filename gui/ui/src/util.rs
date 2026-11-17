//! Utilidades del navegador: hora local, tema y avisos breves.

use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use zeroagents_core::theme::Theme;

/// Hora local de un epoch en segundos: «14:05», o «jue 14:05» si falta más de un día.
pub fn clock(epoch: Option<f64>, now: f64) -> String {
    let Some(epoch) = epoch.filter(|e| *e > 0.0) else {
        return "—".into();
    };
    let d = js_sys::Date::new(&JsValue::from_f64(epoch * 1000.0));
    let hm = format!("{:02}:{:02}", d.get_hours(), d.get_minutes());
    if epoch - now > 86_400.0 {
        let day = ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"][d.get_day() as usize % 7];
        format!("{day} {hm}")
    } else {
        hm
    }
}

/// Hora actual del navegador en epoch (segundos).
pub fn now() -> f64 {
    js_sys::Date::now() / 1000.0
}

/// Vuelca el tema en variables CSS de `:root`: la página cambia sola.
pub fn apply_theme(theme: &Theme) {
    let Some(root) = document().document_element() else { return };
    let style = root.unchecked_ref::<web_sys::HtmlElement>().style();
    for (name, value) in theme.colores.css_vars() {
        let _ = style.set_property(name, value);
    }
    let font = |custom: &Option<String>, var: &str, fallback: &str| match custom {
        Some(f) => {
            let _ = style.set_property(var, &format!("\"{f}\", {fallback}"));
        }
        None => {
            let _ = style.remove_property(var);
        }
    };
    font(&theme.fuentes.sans, "--sans", "\"Inter Tight\", system-ui, sans-serif");
    font(&theme.fuentes.mono, "--mono", "\"JetBrainsMono Nerd Font\", monospace");
    let _ = root.set_attribute("data-modo", if theme.is_light() { "claro" } else { "oscuro" });
}

/// Aviso breve abajo a la derecha.
#[derive(Clone, Copy)]
pub struct Toast(pub RwSignal<Option<(String, bool)>>);

impl Toast {
    pub fn show(self, text: impl Into<String>, error: bool) {
        let text = text.into();
        self.0.set(Some((text.clone(), error)));
        let signal = self.0;
        set_timeout(
            move || {
                if signal.with_untracked(|t| t.as_ref().is_some_and(|(x, _)| *x == text)) {
                    signal.set(None);
                }
            },
            Duration::from_millis(4200),
        );
    }
}
