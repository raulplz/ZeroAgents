//! Puente con el backend: `invoke` para pedir datos y `listen` para recibir
//! los eventos que emite (`board`, `theme`). Usa la API global de Tauri
//! (`app.withGlobalTauri` en tauri.conf.json).

use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_raw(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn listen_raw(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> Result<JsValue, JsValue>;
}

fn error_text(err: JsValue) -> String {
    err.as_string().unwrap_or_else(|| format!("{err:?}"))
}

/// Llama a un comando del backend y convierte la respuesta.
pub async fn invoke<A: Serialize, T: DeserializeOwned>(cmd: &str, args: &A) -> Result<T, String> {
    let args = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    let value = invoke_raw(cmd, args).await.map_err(error_text)?;
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
}

/// Sin argumentos.
#[derive(Serialize)]
pub struct NoArgs {}

/// Escucha un evento del backend durante toda la vida de la app.
pub async fn listen<T: DeserializeOwned + 'static>(event: &str, mut on: impl FnMut(T) + 'static) {
    let handler = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
        let payload = js_sys::Reflect::get(&ev, &JsValue::from_str("payload")).unwrap_or(JsValue::NULL);
        if let Ok(value) = serde_wasm_bindgen::from_value(payload) {
            on(value);
        }
    });
    let _ = listen_raw(event, &handler).await;
    // La ventana vive tanto como la app: el cierre se queda para siempre.
    handler.forget();
}
