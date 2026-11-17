//! Tema ZERØ dinámico.
//!
//! Los temas de ZerØLayer escriben `~/.config/zerolayer/theme.json`. Si no
//! existe, o le falta algún color, se usa la paleta Frost. El tipo se comparte
//! con el frontend, que convierte cada color en una variable CSS.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Colors {
    pub fondo: String,
    pub superficie: String,
    pub borde: String,
    pub texto: String,
    pub tenue: String,
    pub acento: String,
    pub acento2: String,
    pub ok: String,
    pub aviso: String,
    pub peligro: String,
}

impl Default for Colors {
    /// Paleta Frost (obsidian-blue), la de Frost Study y el panel de terminal.
    fn default() -> Self {
        Self {
            fondo: "#080b10".into(),
            superficie: "#111923".into(),
            borde: "#1c2a3a".into(),
            texto: "#d5dfeb".into(),
            tenue: "#8292a6".into(),
            acento: "#4da3ff".into(),
            acento2: "#75c4e8".into(),
            ok: "#8fbea0".into(),
            aviso: "#e6bf79".into(),
            peligro: "#ef7180".into(),
        }
    }
}

impl Colors {
    /// Pares (variable CSS, valor) para aplicar en `:root`.
    pub fn css_vars(&self) -> [(&'static str, &str); 10] {
        [
            ("--fondo", &self.fondo),
            ("--superficie", &self.superficie),
            ("--borde", &self.borde),
            ("--texto", &self.texto),
            ("--tenue", &self.tenue),
            ("--acento", &self.acento),
            ("--acento2", &self.acento2),
            ("--ok", &self.ok),
            ("--aviso", &self.aviso),
            ("--peligro", &self.peligro),
        ]
    }

    /// Rellena los colores vacíos o no válidos con los de Frost.
    fn fill_from(&mut self, base: &Colors) {
        let fields = [
            (&mut self.fondo, &base.fondo),
            (&mut self.superficie, &base.superficie),
            (&mut self.borde, &base.borde),
            (&mut self.texto, &base.texto),
            (&mut self.tenue, &base.tenue),
            (&mut self.acento, &base.acento),
            (&mut self.acento2, &base.acento2),
            (&mut self.ok, &base.ok),
            (&mut self.aviso, &base.aviso),
            (&mut self.peligro, &base.peligro),
        ];
        for (value, fallback) in fields {
            if !is_css_color(value) {
                *value = fallback.clone();
            }
        }
    }
}

/// Solo se aceptan colores `#rgb`, `#rrggbb` o `#rrggbbaa`: el valor acaba en
/// el CSS de la página y no debe poder colar otra cosa.
pub fn is_css_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    pub sans: Option<String>,
    pub mono: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub id: String,
    pub nombre: String,
    /// «oscuro» o «claro».
    pub modo: String,
    pub colores: Colors,
    pub fondo_pantalla: Option<String>,
    pub glifo: Option<String>,
    /// Si viene, se usa como marca de agua decorativa.
    pub kanji: Option<String>,
    pub fuentes: Fonts,
    /// `true` si viene de theme.json; `false` si es la paleta por defecto.
    #[serde(skip_deserializing)]
    pub externo: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            id: "frost".into(),
            nombre: "Frost".into(),
            modo: "oscuro".into(),
            colores: Colors::default(),
            fondo_pantalla: None,
            glifo: None,
            kanji: None,
            fuentes: Fonts::default(),
            externo: false,
        }
    }
}

impl Theme {
    /// Interpreta un theme.json. Los huecos se rellenan con Frost.
    pub fn parse(text: &str) -> Option<Theme> {
        let mut theme: Theme = serde_json::from_str(text).ok()?;
        theme.colores.fill_from(&Colors::default());
        theme.externo = true;
        // Las fuentes también van al CSS: fuera comillas y punto y coma.
        let clean = |f: &mut Option<String>| {
            *f = f.take().map(|s| s.replace(['"', '\'', ';', '{', '}', '<', '>'], "")).filter(|s| !s.trim().is_empty());
        };
        clean(&mut theme.fuentes.sans);
        clean(&mut theme.fuentes.mono);
        if theme.nombre.is_empty() {
            theme.nombre = if theme.id.is_empty() { "ZERØ".into() } else { theme.id.clone() };
        }
        Some(theme)
    }

    pub fn is_light(&self) -> bool {
        matches!(self.modo.to_lowercase().as_str(), "claro" | "light")
    }
}

#[cfg(feature = "native")]
pub mod watch {
    //! Lectura y vigilancia (por sondeo) de `~/.config/zerolayer/theme.json`.
    use std::path::PathBuf;

    use super::Theme;

    pub fn path() -> PathBuf {
        crate::paths::config_dir().join("zerolayer/theme.json")
    }

    pub fn load() -> Theme {
        std::fs::read_to_string(path()).ok().and_then(|t| Theme::parse(&t)).unwrap_or_default()
    }

    /// Recuerda la última versión vista; `poll` devuelve el tema si cambió.
    #[derive(Default)]
    pub struct ThemeWatch {
        last: Option<(Option<f64>, u64)>,
    }

    impl ThemeWatch {
        pub fn poll(&mut self) -> Option<Theme> {
            let p = path();
            let stamp = (crate::store::mtime(&p), std::fs::metadata(&p).map_or(0, |m| m.len()));
            if self.last == Some(stamp) {
                return None;
            }
            self.last = Some(stamp);
            Some(load())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_tema_parcial_se_completa_con_frost() {
        let t = Theme::parse(r##"{"id":"sakura","nombre":"Sakura","modo":"claro",
            "colores":{"fondo":"#fff5f8","acento":"#e05a8a","peligro":"red; background:url(x)"},
            "kanji":"桜","fuentes":{"sans":"Inter Tight\"; x"}}"##)
        .unwrap();
        assert_eq!(t.colores.fondo, "#fff5f8");
        assert_eq!(t.colores.acento, "#e05a8a");
        assert_eq!(t.colores.texto, "#d5dfeb", "faltaba: Frost");
        assert_eq!(t.colores.peligro, "#ef7180", "no válido: Frost");
        assert_eq!(t.kanji.as_deref(), Some("桜"));
        assert_eq!(t.fuentes.sans.as_deref(), Some("Inter Tight x"));
        assert!(t.is_light() && t.externo);
    }

    #[test]
    fn sin_tema_frost() {
        let t = Theme::default();
        assert_eq!(t.colores.acento, "#4da3ff");
        assert!(!t.externo && !t.is_light());
        assert!(Theme::parse("no es json").is_none());
        assert!(is_css_color("#abc") && !is_css_color("abc") && !is_css_color("#ggg"));
    }
}
