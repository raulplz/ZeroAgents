# ❄ ZeroAgents

> *A Frost-styled terminal dashboard to monitor AI coding agents (Claude Code) and to turn a project prompt into a team of agents, each with its own model and effort, led by a Product Owner.*

Panel Frost para vigilar a la vez varios agentes de IA. De momento solo cubre **Claude Code**: muestra el uso, el modelo, el esfuerzo y el modo de cada sesión, y avisa cuando alguna necesita algo de ti. Está pensado para que otras plataformas entren como adaptadores (ver *Añadir una plataforma*).

## Qué muestra

| Columna | De dónde sale |
|---|---|
| Estado: `PERMISO`, `PREGUNTA`, `trabajando`, `tu turno` | hooks + registro de sesiones |
| Modelo, esfuerzo | statusline (en vivo) o transcript |
| Modo (normal, editar, plan, auto, sin permisos…) | hooks o transcript |
| Contexto %, coste estimado, duración | statusline |
| Tokens (entrada nueva + salida; la caché, aparte en el detalle) | transcript |
| Límites de la cuenta 5 h / 7 d | statusline |

Las sesiones que te necesitan suben arriba y aparecen en rojo con el motivo (p. ej. `Bash: Borrar la carpeta build`). Además, llega una notificación de escritorio cuando Claude lleva unos segundos esperando un permiso o una respuesta, o un minuto esperando tu siguiente mensaje.

El coste es la **estimación a precio de lista** que calcula Claude Code. Con suscripción no es lo que pagas; lo útil ahí son los límites de 5 h y 7 d.

## Uso

```bash
zeroagents            # panel (q salir · f/Enter ir a su ventana · a ver cerradas · j/k moverse)
zeroagents list       # tabla en texto; --json para scripts; --all incluye cerradas (24 h)
zeroagents doctor     # comprueba la instalación
```

## Modo Work: de un prompt a un equipo de agentes

Describes un proyecto y un **arquitecto de equipos** diseña dos cosas: el equipo de agentes que lo construirá y el planteamiento del proyecto. Cada agente lleva su propio **modelo y esfuerzo** según lo que tenga que hacer. Después, el líder del equipo, que hace de **Product Owner**, se abre en una terminal y el panel sigue el progreso. Tú haces de **cliente**: contestas las dudas y apruebas los puntos de control.

### Guía rápida: desde cero

**1. Abrir el panel en modo Work**

Abre **ZeroAgents** desde el lanzador de aplicaciones, que arranca ya en el modo Work. Para que aparezca, copia la plantilla: `cp extras/zeroagents.desktop ~/.local/share/applications/`. Desde una terminal también vale `zeroagents --work`, o `zeroagents` y después `w`.

**2. Crear un proyecto**

1. Pulsa `n` y rellena el formulario. Para moverte entre los campos usa `Tab`.
   - **Descripción:** qué es, para quién y las tecnologías o límites que quieras. Cuanto más concreto, mejor equipo.
   - **Carpeta:** si la dejas vacía, se crea en `~/Work/PROYECTOS/<nombre>`.
   - **Modelos:** por defecto `haiku,sonnet,opus`. Para algo barato, pon `haiku,sonnet`.
   - **Arquitecto de equipos:** *Rápido* (Sonnet, esfuerzo medio, unos 30 s) o *Calidad* (Opus, esfuerzo alto, 1-3 min y más caro).
   - **Permisos del equipo al arrancar:** *Aceptar ediciones de archivos* (Bash sigue preguntando) o *Preguntar todo*.
2. Pulsa «Diseñar equipo». Arriba aparece «◐ arquitecto diseñando…».
3. Cuando termina, el proyecto aparece en la lista. Pulsa `p` para **leer el plan** dentro de la app: el equipo, las tareas y las dudas abiertas.

**3. Arrancar el equipo**

1. Selecciona el proyecto y pulsa `l`. Se abre una ventana con el Product Owner, con los permisos que elegiste en el formulario.
2. Si Claude pregunta si confías en la carpeta, di que sí.
3. Te resume el plan, los roles y las dudas, y crea el equipo: a cada compañero le dice su rol y de qué archivos es dueño.
4. Haz de cliente: contesta las dudas y aprueba o corrige en cada punto de control. Al terminar los requisitos se para y te pregunta.

**4. Seguirlo**

| Tecla | Qué hace |
|---|---|
| `f` | Ir a la ventana del Product Owner (cuando sale el aviso rojo «te necesita») |
| `p` | Ver el plan (`docs/PLAN.md`) dentro de la app |
| `m` | Ver en vivo los **mensajes entre agentes**: cada encargo del líder y cada entrega (`c` alterna los textos completos) |
| `o` | Abrir la carpeta del proyecto en VS Code, para ver los archivos que crea el equipo |
| `Tab` | Cambiar entre las tablas (proyectos, equipo y tareas) |
| `↑` `↓` / `j` `k` | Moverse por la tabla |
| `r` | Refrescar (se refresca solo cada 1,5 s) |
| `w` / `Esc` | Volver a la vista de sesiones |
| `q` | Salir del panel. El equipo sigue trabajando |

Los atajos se ven siempre en la barra inferior. Los permisos que piden los compañeros aparecen **en la ventana del Product Owner**, no en el panel.

**Lo mismo por terminal**

```bash
zeroagents work new "Una app de notas en Python con SQLite y tests" -d ~/Work/PROYECTOS/notas
zeroagents work start ~/Work/PROYECTOS/notas --permission-mode acceptEdits
zeroagents work show ~/Work/PROYECTOS/notas   # equipo, tareas y progreso en texto
zeroagents work list                          # todos los proyectos
zeroagents messages -f ~/Work/PROYECTOS/notas # mensajes entre agentes en vivo
```

- **Opciones de `work new`:** `--models haiku,sonnet` limita los modelos del equipo, y `--architect-model sonnet --architect-effort medium` hace el diseño más barato.
- **Permisos:** `--permission-mode acceptEdits` acepta solas las ediciones de archivos; Bash y lo demás siguen preguntando. Sin esa opción, `work start` usa lo que se eligió al crear el proyecto.

**5. Terminar**

Escribe `/exit` en la ventana del Product Owner o ciérrala, y el equipo se detiene.
- La configuración del equipo desaparece al cerrar la sesión; sus tareas se conservan.
- El proyecto sigue en la lista mientras exista su carpeta, y se puede volver a arrancar con `l`.

**Qué hace cada paso**

1. **Arquitecto** (`work/architect.py`): `claude -p --json-schema` con Opus y esfuerzo alto (`--architect-model` y `--architect-effort` para cambiarlo).
   - Solo puede leer la carpeta (Read, Glob y Grep). Así planifica sobre el código que ya exista, pero no escribe nada.
   - Sigue el ciclo de vida **requisitos → arquitectura → implementación → QA**, con un agente por responsabilidad (puede haber varios desarrolladores o dos arquitectos).
   - A cada agente le asigna modelo y esfuerzo, y justifica la elección por coste. Por defecto puede usar `haiku`, `sonnet` y `opus`; `--models haiku,sonnet,opus,fable` permite también `fable`.
   - Devuelve las tareas con sus dependencias, los puntos de control y las dudas abiertas. ZeroAgents comprueba la coherencia: responsables que existen, nombres válidos y dependencias sin ciclos.
2. **Archivos** (`work/scaffold.py`). **Nunca sobrescribe**: si un archivo ya existe, lo deja.
   - `.claude/agents/<agente>.md`: un subagente por rol, con `model`, `effort`, `tools` y su system prompt, más un «contrato» que dice qué lee y de qué es dueño.
   - `docs/PLAN.md`: el planteamiento, el equipo, las tareas y los puntos de control.
   - `docs/EQUIPO.md`: reglas comunes y dueños de cada artefacto.
   - `docs/decisiones.md`: registro de decisiones.
   - `.zeroagents/lead_prompt.md`: instrucciones para el líder.
   - `.zeroagents/team.json`: el plan que lee el panel. Si ya había uno, antes se guarda una copia con la fecha.
3. **Líder** (`work/launch.py`): abre `claude --model <m> --effort <e>` con la variable de entorno `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`.
   - La variable se pone **solo para ese proceso**: no toca `~/.claude/settings.json`.
   - El líder crea a cada compañero **a partir de su tipo de agente**, así hereda su modelo, su esfuerzo y sus herramientas.
   - Se detiene para pedir tu aprobación tras los requisitos y tras la arquitectura.
4. **Seguimiento** (`work/live.py`): junta el plan, la sesión del líder (si te necesita, lo verás en rojo) y el estado interno del equipo de Claude Code (`~/.claude/teams/<equipo>/config.json` y `~/.claude/tasks/<equipo>/`).
   - Ese formato **no está documentado**, así que se lee de forma tolerante.
   - Mientras el equipo no ha creado tareas, el panel muestra las del plan.

**A tener en cuenta**

- Los equipos de Claude Code son **experimentales** y gastan bastantes más tokens que una sesión normal.
- Los permisos que piden los compañeros aparecen en la sesión del líder.
- Para escribir a un compañero concreto se hace desde Claude: en el modo integrado, flechas en el panel de agentes, Enter y escribir.
- El arquitecto cuesta unos 0,2 $ con Sonnet a precio de lista (prueba del 10-11-2026). Con Opus, más.

## Mensajes entre agentes

ZeroAgents guarda cada mensaje que se pasan los agentes, con el texto íntegro, a partir de los mismos hooks que ya instala:

| Tipo | Cuándo | De dónde sale |
|---|---|---|
| **ENCARGO** | El agente principal delega en un subagente | `PreToolUse` de `Agent`: `prompt`, `description` y `subagent_type` |
| **ENTREGA** | El subagente termina | `PostToolUse` de `Agent` (primer plano), `SubagentStop` (segundo plano) o `SubagentHandback` (modo auto) |
| **MENSAJE** | Un agente escribe a otro (equipos, otras sesiones) | `PreToolUse` de `SendMessage` |

Los agentes internos de Claude Code (sugerencias de prompt, `/btw`) también disparan `SubagentStop`, así que solo se cuentan las entregas de subagentes lanzados antes en segundo plano.

```bash
zeroagents messages            # en un proyecto con .zeroagents/, su registro; si no, las últimas 24 h
zeroagents messages -f         # en vivo (Ctrl+C para salir)
zeroagents messages --full     # textos completos
zeroagents messages --md > docs/registro_prompts.md   # registro en Markdown
zeroagents messages --json     # una línea JSON por mensaje
```

Dónde se guardan:
- `~/.local/state/zeroagents/claude-code/<sesión>.messages.jsonl`, como el resto del estado (se borra a los 7 días).
- `.zeroagents/mensajes.jsonl` dentro del proyecto, si existe la carpeta `.zeroagents/` (los proyectos del modo Work la tienen; en otro proyecto basta con crearla). Ese archivo es permanente y sirve de evidencia de cómo trabajó el equipo.

Cada texto se recorta a 50 000 caracteres.

## Instalación

```bash
cd ~/Work/PROYECTOS/ZeroAgents
uv tool install --editable .   # deja `zeroagents` en ~/.local/bin (editable: los cambios aplican solos)
zeroagents install             # hooks + statusline en ~/.claude/settings.json (con copia previa)
```

`install` añade hooks **asíncronos** (unos 50 ms, sin bloquear a Claude) y envuelve tu statusline, que se sigue viendo igual. Opciones: `--no-notify` (sin notificaciones) y `--no-statusline` (sin envolverla; se pierden coste, contexto y límites). Las copias de `settings.json` quedan en `~/.local/state/zeroagents/backups/`.

Para quitarlo: `zeroagents uninstall` (elimina los hooks y restaura la statusline original), y después `uv tool uninstall zeroagents` si quieres borrar también el comando.

## Cómo funciona

```
Claude Code ──hooks async──▶ zeroagents hook ──▶ ~/.local/state/zeroagents/claude-code/<sesión>.events.jsonl
            ──statusline───▶ zeroagents statusline ──▶ <sesión>.status.json  (y pinta tu barra)
~/.claude/sessions/<pid>.json  (sesiones vivas, ocupada/libre)  ┐
~/.claude/projects/…/<sesión>.jsonl (tokens, respaldo)         ┴─▶ panel / list
```

- Los eventos se añaden línea a línea, sin bloqueos, así que varios hooks simultáneos no se pisan.
- Una petición de permiso de un subagente no se borra cuando el agente principal sigue trabajando.
- Pulsar Esc ante un permiso no dispara ningún hook. Para detectarlo, el registro de sesiones lo marca como «libre».
- El registro `~/.claude/sessions/` y el formato del transcript son **internos** de Claude Code y pueden cambiar. Los hooks y la statusline sí están documentados (https://code.claude.com/docs/en/hooks y https://code.claude.com/docs/en/statusline).
- El estado propio con más de 7 días se borra solo. Nunca toca los datos de Claude.

Limitación conocida: si cambias de modo con Shift+Tab, el panel no lo refleja hasta el siguiente evento (mensaje o herramienta), porque Claude Code no avisa de ese cambio.

## Añadir una plataforma

1. Crea `src/zeroagents/adapters/<plataforma>.py` con una clase que herede de `Adapter` y devuelva `Agent`/`Limits` (en `model.py`).
2. Regístrala en `ADAPTERS` (`adapters/__init__.py`) y añade su nombre corto en `fmt.PLATFORMS`.

## Desarrollo

```bash
uv run pytest -q   # lógica de estados, transcript, nombres de modelo
uv run zeroagents  # usa el código del proyecto
ZEROAGENTS_STATE_DIR=/ruta/temporal zeroagents list   # probar sin mezclar estado real
```

## Estado

**v0.3** · 33 pruebas automáticas (`uv run pytest -q`).

- **Monitor de sesiones:** adaptador de Claude Code, panel TUI, `list`/`--json`, notificaciones de escritorio, saltar a la ventana del agente (Hyprland) e `install`/`uninstall`/`doctor`.
- **Mensajes entre agentes** (v0.3): encargos, entregas y mensajes con su texto íntegro, en vivo en el panel (`m`) y por terminal (`messages -f`), con registro permanente por proyecto.
- **Modo Work:** arquitecto de equipos con salida validada por esquema, generación del proyecto (agentes con modelo y esfuerzo propios, plan, reglas y prompt del Product Owner), arranque del líder y panel de seguimiento.
- **Probado:** el arquitecto genera equipos coherentes en 30-60 s, y Claude Code reconoce los agentes generados.
- **Experimental:** los equipos de Claude Code lo son, y el formato interno de sus tareas no está documentado. El panel lo lee de forma tolerante y puede necesitar ajustes.

**Próximo:** responder permisos desde el panel (hook `PermissionRequest`), eventos de tareas del equipo (`TaskCreated`/`TaskCompleted`/`TeammateIdle`) y un adaptador para Codex.

## Licencia

MIT
