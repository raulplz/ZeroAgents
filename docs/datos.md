# Datos compartidos entre la terminal y la GUI

La terminal (Python, `src/zeroagents`) y la GUI (Rust, `gui/core`) leen y escriben **las mismas rutas con el mismo formato**. Se puede instalar una, la otra o las dos: no hay copias de datos ni conversión.

## Rutas

| Qué | Ruta | Quién escribe | Quién lee |
|---|---|---|---|
| Carpeta de estado | `$ZEROAGENTS_STATE_DIR`, o `$XDG_STATE_HOME/zeroagents`, o `~/.local/state/zeroagents` | | |
| Eventos de hooks | `claude-code/<sesión>.events.jsonl` | hooks (`zeroagents hook`), solo añadir | TUI, GUI |
| Mensajes entre agentes | `claude-code/<sesión>.messages.jsonl` y `<proyecto>/.zeroagents/mensajes.jsonl` | hooks, solo añadir | TUI, GUI |
| Statusline | `claude-code/<sesión>.status.json` | `zeroagents statusline`, reemplazo atómico | TUI, GUI |
| Libro de subagentes | `claude-code/<sesión>.agents.json` | hooks (`flock`) | solo Python |
| Registro de proyectos Work | `work/projects.json` (+ `.lock`) | TUI y GUI (`flock`) | TUI, GUI |
| Plan del equipo | `<proyecto>/.zeroagents/team.json` | arquitecto (Python), atómico | TUI, GUI |
| Copia de la statusline original | `install.json` | `zeroagents install` | `zeroagents uninstall` |
| Copias de `settings.json` | `backups/` | `zeroagents install/uninstall` | |
| Sesiones de Claude Code, equipos, tareas | `~/.claude/` (`CLAUDE_CONFIG_DIR`) | Claude Code | TUI, GUI (solo lectura) |
| Tema | `~/.config/zerolayer/theme.json` | ZerØLayer | GUI (solo lectura) |

No hay archivo de configuración propio, ni sockets, ni demonio: las dos interfaces son clientes del mismo estado. Los procesos que escriben son los **hooks de Claude Code** (muchos y simultáneos), `zeroagents install/uninstall` y `work new/start` (Python), y `work::register` (Rust, para cuando la GUI añada proyectos). El estado con más de 7 días lo borra cualquiera de las dos al arrancar (es idempotente).

La **ingesta** (hooks y statusline) es solo Python: la GUI necesita el comando `zeroagents` instalado para tener datos. Por eso `install.sh gui` lo instala si falta y `uninstall.sh tui` lo conserva mientras la GUI esté instalada.

## Reglas de escritura (las dos implementaciones)

1. **Atómica**: se escribe un temporal `.<nombre>.<pid>.tmp` en la misma carpeta, `fsync` y `rename`. Un lector ve el contenido anterior completo o el nuevo completo.
2. **Bloqueo**: lo que se lee y se reescribe (`projects.json`, `*.agents.json`) toma `flock` exclusivo sobre `<archivo>.lock` (`fcntl.flock` en Python, `File::lock` en Rust: la misma llamada del sistema, así que se excluyen entre sí). Dentro del bloqueo se **vuelve a leer** el archivo, de modo que nunca se pisa un cambio ajeno.
3. **Campos desconocidos**: se reescribe sobre lo leído. En Rust cada tipo lleva `#[serde(flatten)] extra` (y los proyectos van como objetos JSON sin tipar); en Python se modifica el dict leído. Un campo que añada una versión futura sobrevive a las dos.
4. **Archivo ilegible**: si existe pero no es JSON válido, se copia a `<archivo>.corrupto-<fecha>` antes de crear uno nuevo; nunca se pierde en silencio.
5. **Migraciones**: solo hacia delante y solo añaden. Antes de migrar se guarda `<archivo>.bak` (una vez: la copia es el original). Si `version` es mayor que la que conoce el programa, se respeta tal cual y no se rebaja.
6. **Solo añadir** para los `.jsonl`: una línea por `write` en modo append (atómico para líneas cortas). Los lectores (`JsonlTail`, en ambos lenguajes) conservan el desplazamiento y esperan a que la última línea esté completa.

## Esquemas

### `work/projects.json` — versión 1

```json
{
  "version": 1,
  "projects": [
    { "dir": "/ruta/al/proyecto", "name": "Nombre", "added": 1794951323.5 }
  ]
}
```

- `projects`: el más reciente primero. Registrar de nuevo un proyecto lo sube arriba y actualiza `name` y `added`, conservando cualquier otro campo suyo.
- Los elementos que no son objetos o no tienen `dir` se ignoran al leer y se conservan al escribir.
- **v0 → v1**: el archivo original no tenía `version`; la primera escritura la añade (con `.bak`).

### `<proyecto>/.zeroagents/team.json` — versión `zeroagents: 1`

`{"zeroagents": 1, "created": <epoch>, ..., "plan": {...}}`. La escribe el arquitecto de Python (si había uno, antes se renombra a `team.<fecha>.json`) y la GUI solo lo lee, tolerando campos que no conoce (`#[serde(default)]`).

### `claude-code/<sesión>.agents.json`

`{"bg": ["<id>", ...], "handed": ["<id>", ...]}`. Altas y bajas se aplican bajo bloqueo sobre lo leído en ese momento, porque los hooks asíncronos de varios subagentes coinciden.

### `status.json`, `events.jsonl`, `messages.jsonl`

Los produce Python y los dos leen; ambos ignoran campos que no conocen. Claves de los eventos: `t` (epoch), `e` (evento), y opcionales `tool`, `msg`, `pm`, `eff`, `model`, `cwd`, `tp`, `nt`, `ag`, `last`. Mensajes: `t`, `kind` (`encargo`/`entrega`/`mensaje`), `sesion`, `de`, `para`, `titulo`, `texto`, `via`.

## Incompatibilidades que había y cómo quedaron

| Problema | Arreglo |
|---|---|
| `register()` leía y reescribía `projects.json` sin bloqueo: dos altas a la vez perdían una | `update_json` / `jsonfile::update` con `flock` y relectura |
| `register()` descartaba campos de nivel superior y los del propio proyecto | se reescribe sobre lo leído |
| Un `projects.json` ilegible se sobrescribía sin aviso; un elemento no-objeto rompía `projects()` en Python | copia `.corrupto-<fecha>` y lectura tolerante |
| `projects.json` sin versión | `version: 1` con migración y `.bak` |
| `team.json`, `install.json` y `settings.json` se escribían con `write_text` (un lector podía ver el archivo a medias) | `write_atomic` (tmp + `fsync` + `rename`); `install`/`uninstall` bajo `install.lock` |
| El libro de subagentes (`*.agents.json`) perdía altas con hooks simultáneos | operaciones bajo bloqueo con relectura |
| `XDG_STATE_HOME` o `CLAUDE_CONFIG_DIR` vacíos: Python usaba `.`, Rust la ruta por defecto | Python trata vacío como no definido |
| La GUI no escribía nada, pero no tenía forma segura de hacerlo | `jsonfile` (bloqueo, atómico, `extra`) y `work::register` |

Pendiente conocido: los `.lock` de la carpeta de cada sesión se borran con el resto del estado a los 7 días; solo importaría si un hook coincidiera justo con esa limpieza.

## Pruebas

- `uv run pytest -q`: `tests/test_compat.py` usa el ejemplo `compat_helper` de Rust (`cd gui && cargo build -p zeroagents-core --example compat_helper`; se omite si no está compilado).
- `cd gui && cargo test -p zeroagents-core --test compat`: lanza Python real (`tests/compat_helper.py`) sobre un `HOME` aislado. Cubre: Python escribe y Rust lee (mismos estado, modelo, coste, límites), registro cruzado con campos desconocidos, migración v0→v1, archivo corrupto, escrituras simultáneas de hilos de Rust y procesos de Python con un lector sin bloqueo, y que el `flock` de Rust bloquea a Python.
