//! Tema ZERØ dinámico.
//!
//! Los temas de ZerØLayer escriben `~/.config/zerolayer/theme.json`. La lectura, la validación y la
//! paleta Frost de respaldo viven en el crate `zerolayer-theme`; aquí solo se fija cómo se llaman las
//! variables CSS. El tipo se comparte con el frontend, que vuelca cada valor en `:root`.

use zerolayer_theme::CssMap;
pub use zerolayer_theme::Theme;

/// Variables CSS de ZeroAgents: `--fondo`, `--superficie`, `--borde`, `--texto`, `--tenue`, `--acento`,
/// `--acento2`, `--ok`, `--aviso`, `--peligro`, `--sans` y `--mono`.
pub fn css_map() -> CssMap {
    CssMap::default().fallbacks("system-ui, sans-serif", "monospace")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_con_los_nombres_de_zeroagents() {
        let t: Theme = serde_json::from_str(r##"{"id":"sakura","modo":"light","colores":{"fondo":"#fff5f8","peligro":"red; x"},"fuentes":{"sans":"Inter Tight\"; x"}}"##).unwrap();
        let v = t.css_variables(&css_map());
        let get = |k: &str| v.iter().find(|(n, _)| *n == k).map(|(_, x)| x.as_str());
        assert_eq!(get("--fondo"), Some("#fff5f8"));
        assert_eq!(get("--peligro"), Some("#ef7180"), "no válido: Frost");
        assert_eq!(
            get("--sans"),
            Some("\"Inter Tight x\", system-ui, sans-serif")
        );
        assert!(t.is_light());
        assert_eq!(v.len(), 12);
    }
}
