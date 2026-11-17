//! Saltar a la ventana de terminal de un agente (port de `focus.py`; de momento, solo Hyprland).

use std::collections::HashMap;
use std::fs;
use std::process::Command;

use serde::Deserialize;

/// PID del padre, leído de `/proc/<pid>/stat`.
fn ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

#[derive(Deserialize)]
struct Client {
    pid: i64,
    address: String,
}

/// Sube por los padres del proceso hasta dar con uno que tenga ventana.
pub fn window_for(pid: u32, windows: &HashMap<u32, String>) -> Option<String> {
    let mut current = Some(pid);
    while let Some(p) = current.filter(|p| *p > 1) {
        if let Some(address) = windows.get(&p) {
            return Some(address.clone());
        }
        current = ppid(p);
    }
    None
}

/// Enfoca la ventana que contiene el proceso. `Err` con el motivo si no puede.
pub fn focus_pid(pid: Option<u32>) -> Result<(), String> {
    let Some(pid) = pid.filter(|p| *p > 0) else {
        return Err("Este agente no tiene proceso asociado.".into());
    };
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return Err("Saltar a la ventana solo funciona en Hyprland por ahora.".into());
    }
    let windows: HashMap<u32, String> = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .and_then(|out| serde_json::from_slice::<Vec<Client>>(&out.stdout).ok())
        .ok_or("No he podido consultar las ventanas de Hyprland.")?
        .into_iter()
        .filter_map(|c| u32::try_from(c.pid).ok().map(|p| (p, c.address)))
        .collect();
    let address = window_for(pid, &windows)
        .ok_or("No encuentro la ventana de este agente (¿está en tmux o en otra sesión?).")?;
    focus_address(&address)
}

fn focus_address(address: &str) -> Result<(), String> {
    // Hyprland con configuración Lua usa hl.dsp.*; las versiones anteriores, la sintaxis clásica.
    let lua = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    let classic = format!("address:{address}");
    let attempts: [Vec<&str>; 2] =
        [vec!["dispatch", lua.as_str()], vec!["dispatch", "focuswindow", classic.as_str()]];
    for args in attempts {
        if let Ok(out) = Command::new("hyprctl").args(&args).output()
            && out.status.success()
            && String::from_utf8_lossy(&out.stdout).trim() == "ok"
        {
            return Ok(());
        }
    }
    Err("Hyprland no ha aceptado la orden de enfocar la ventana.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encuentra_la_ventana_del_propio_proceso_o_de_un_padre() {
        let me = std::process::id();
        let parent = ppid(me).unwrap();
        let windows = HashMap::from([(parent, "0xabc".to_string())]);
        assert_eq!(window_for(me, &windows).as_deref(), Some("0xabc"));
        assert_eq!(window_for(me, &HashMap::new()), None);
    }

    #[test]
    fn sin_pid_no_hay_ventana() {
        assert!(focus_pid(None).is_err());
    }
}
