//! Formato compartido por el backend y el frontend (port de `fmt.py`).
//!
//! Todo es puro: las funciones que dependen de la hora reciben `now`, así
//! también funcionan en wasm y se pueden probar.

/// Modo de permisos de Claude Code, en español.
pub fn mode(value: Option<&str>) -> String {
    match value.unwrap_or("") {
        "" => "—".into(),
        "default" | "manual" => "normal".into(),
        "acceptEdits" => "editar".into(),
        "plan" => "plan".into(),
        "auto" => "auto".into(),
        "dontAsk" => "no preguntar".into(),
        "bypassPermissions" => "sin permisos".into(),
        other => other.into(),
    }
}

pub fn effort(value: Option<&str>) -> String {
    match value.unwrap_or("") {
        "" => "—".into(),
        "low" => "bajo".into(),
        "medium" => "medio".into(),
        "high" => "alto".into(),
        "xhigh" => "muy alto".into(),
        "max" => "máximo".into(),
        other => other.into(),
    }
}

pub fn platform(value: &str) -> &str {
    match value {
        "claude-code" => "Claude",
        other => other,
    }
}

/// `claude-opus-5-5` → `Opus 5.5`; `claude-sonnet-5-5[1m]` → `Sonnet 5.5 · 1M`.
pub fn pretty_model(model_id: Option<&str>) -> Option<String> {
    let model_id = model_id?;
    if model_id.is_empty() {
        return None;
    }
    let big = model_id.contains("[1m]");
    let core = model_id.replace("[1m]", "");
    let core = core.strip_prefix("claude-").unwrap_or(&core);
    let is_digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    // Quita las fechas de versión (8 dígitos), como 20251001.
    let parts: Vec<&str> = core
        .split('-')
        .filter(|p| !(is_digits(p) && p.len() == 8))
        .collect();
    let first = parts.first().copied().unwrap_or("");
    if first.is_empty() || !first.chars().all(char::is_alphabetic) {
        return Some(model_id.to_string());
    }
    let mut family = first.to_lowercase();
    if let Some(c) = family.get(0..1) {
        family = c.to_uppercase() + &family[1..];
    }
    let version: Vec<&str> = parts[1..].iter().copied().filter(|p| is_digits(p)).collect();
    let name = format!("{family} {}", version.join(".")).trim().to_string();
    Some(if big { format!("{name} · 1M") } else { name })
}

pub fn model(value: Option<&str>) -> String {
    pretty_model(value).unwrap_or_else(|| "—".into())
}

/// 1234 → `1.2k`; 2_000_000 → `2M`.
pub fn tokens(n: u64) -> String {
    if n == 0 {
        return "—".into();
    }
    for (size, suffix) in [(1e9, "G"), (1e6, "M"), (1e3, "k")] {
        if n as f64 >= size {
            return format!("{:.1}{suffix}", n as f64 / size).replace(".0", "");
        }
    }
    n.to_string()
}

pub fn cost(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("${v:.2}"))
}

/// Duración en segundos como «45s», «12m», «3h 05m» o «2d».
pub fn span(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d", s / 86400),
    }
}

/// Hace cuánto: «—» si no hay fecha.
pub fn ago(epoch: Option<f64>, now: f64) -> String {
    match epoch {
        Some(t) if t > 0.0 => span(now - t),
        _ => "—".into(),
    }
}

pub fn duration(ms: Option<u64>) -> String {
    match ms {
        Some(ms) if ms > 0 => span(ms as f64 / 1000.0),
        _ => "—".into(),
    }
}

/// Acorta la carpeta personal a `~`.
pub fn place(cwd: &str, home: &str) -> String {
    if cwd.is_empty() {
        return "—".into();
    }
    match cwd.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => cwd.into(),
    }
}

pub fn pct(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |p| format!("{p:.0}%"))
}

/// Nivel de alarma de un porcentaje: 0 normal, 1 aviso (≥60), 2 peligro (≥85).
pub fn level(value: Option<f64>) -> u8 {
    match value.unwrap_or(0.0) {
        p if p >= 85.0 => 2,
        p if p >= 60.0 => 1,
        _ => 0,
    }
}

/// Recorta por caracteres (no bytes) y añade «…».
pub fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.into();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_model_como_en_python() {
        let cases = [
            (Some("claude-opus-5-5"), Some("Opus 5.5")),
            (Some("claude-sonnet-5-5[1m]"), Some("Sonnet 5.5 · 1M")),
            (Some("claude-haiku-4-5-20251001"), Some("Haiku 4.5")),
            (Some("claude-fable-5-1"), Some("Fable 5.1")),
            (Some("gpt-5"), Some("Gpt 5")),
            (Some("<synthetic>"), Some("<synthetic>")),
            (None, None),
        ];
        for (raw, pretty) in cases {
            assert_eq!(pretty_model(raw).as_deref(), pretty, "{raw:?}");
        }
    }

    #[test]
    fn tokens_y_coste() {
        assert_eq!(tokens(0), "—");
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(1234), "1.2k");
        assert_eq!(tokens(2_000_000), "2M");
        assert_eq!(cost(Some(1.234)), "$1.23");
        assert_eq!(cost(None), "—");
    }

    #[test]
    fn tiempos() {
        assert_eq!(span(45.0), "45s");
        assert_eq!(span(125.0), "2m");
        assert_eq!(span(3.0 * 3600.0 + 5.0 * 60.0), "3h 05m");
        assert_eq!(span(2.0 * 86400.0), "2d");
        assert_eq!(ago(None, 100.0), "—");
        assert_eq!(ago(Some(40.0), 100.0), "1m");
        assert_eq!(duration(Some(90_000)), "1m");
    }

    #[test]
    fn modos_y_carpetas() {
        assert_eq!(mode(Some("acceptEdits")), "editar");
        assert_eq!(mode(None), "—");
        assert_eq!(effort(Some("xhigh")), "muy alto");
        assert_eq!(place("/home/ana/Work/x", "/home/ana"), "~/Work/x");
        assert_eq!(place("/home/anabel", "/home/ana"), "/home/anabel");
        assert_eq!(clip("áéíóúáéíóú", 5), "áéíó…");
        assert_eq!(level(Some(90.0)), 2);
        assert_eq!(level(Some(60.0)), 1);
        assert_eq!(level(None), 0);
    }
}
