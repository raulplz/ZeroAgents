"""El agente «arquitecto de equipos»: de un prompt de proyecto a un plan validado.

Se ejecuta con `claude -p --json-schema`, así que la respuesta siempre llega con
la forma de `plan.schema()`. Solo puede leer (Read/Glob/Grep) la carpeta del
proyecto, por si ya existe código; no escribe nada: los archivos los crea
`scaffold` a partir del JSON.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

from zeroagents.work import plan as P

SYSTEM_PROMPT = """\
Eres el ARQUITECTO DE EQUIPOS de ZeroAgents. Recibes la descripción de un proyecto de software
y diseñas el equipo de agentes de IA que lo construirá con los equipos de Claude Code, además del
planteamiento del proyecto. No escribes código ni archivos: devuelves solo el JSON pedido.

## Ciclo de vida que debe seguir el equipo
1. Toma de requisitos: definición de requisitos y documentación (docs/requisitos.md).
2. Diseño de arquitectura: arquitectura de infraestructura (docs/infraestructura.md, docker-compose.yml…)
   y arquitectura del software (docs/arquitectura.md, contrato de API como api/openapi.yaml si hay API).
3. Implementación: desarrollo de código y refactorización.
4. Pruebas QA: unitarias, E2E y UAT, y documentación final (README, informe de pruebas).

## Cómo diseñar el equipo
- Un agente por RESPONSABILIDAD, no por fase. Puede haber varios desarrolladores (backend, frontend,
  datos/IA…) o dos arquitectos (infraestructura y software) si el proyecto lo justifica. Ni más agentes
  de los necesarios ni menos: un proyecto pequeño puede tener 3-4; uno grande, 6-9.
- No incluyas al líder: es el PRODUCT OWNER del equipo (dueño del backlog, prioriza por valor, define y
  comprueba los criterios de aceptación, acepta o rechaza entregas y coordina). Lo crea ZeroAgents; tú
  eliges su modelo y esfuerzo en "lead". El humano es el cliente (stakeholder): aprueba los puntos de control.
- Cada agente tiene un rol claro (title) y responsabilidades concretas (description + outputs). Los
  que construyen son los developers del equipo, cada uno en su especialidad.
- Quien prueba no es quien programa. Si hay datos personales, credenciales o red, incluye seguridad.
- Cada artefacto tiene UN dueño (outputs). Los demás solo lo leen (inputs). Así no se pisan.
- Rutas de artefactos relativas a la raíz del proyecto.

## Modelo y esfuerzo de cada agente (decisión de coste)
Modelos de menor a mayor capacidad y coste: haiku < sonnet < opus{fable_note}.
Esfuerzo (razonamiento): low < medium < high < xhigh < max; más esfuerzo = más tokens y más tiempo.
- haiku: tareas mecánicas y bien acotadas (formatear, documentar a partir de algo ya decidido,
  pruebas repetitivas). Esfuerzo low o medium.
- sonnet: implementación y QA normales. Esfuerzo medium (high si la lógica es delicada).
- opus: decisiones que condicionan todo lo demás (requisitos ambiguos, arquitectura, revisión de
  seguridad, refactorización compleja). Esfuerzo high; xhigh/max solo si lo justificas.
- El Product Owner prioriza, acepta entregas y decide cuándo pasar de fase: normalmente opus/high o sonnet/high.
Justifica cada elección en "why" en una frase concreta. No pongas todo en el modelo más caro.
Modelos permitidos esta vez: {models}.

## System prompt de cada agente
Escríbelo en español, en segunda persona, con estas piezas: rol, objetivo, contexto del proyecto,
entradas (qué lee), pasos, salidas con formato, restricciones (qué NO hace), criterio de terminado y
qué hacer si falta información (preguntar al líder o anotar la suposición en docs/decisiones.md, nunca
inventar). Herramientas: solo las que necesita (un analista no necesita Bash).

## Tareas y puntos de control
- Tareas pequeñas y comprobables (T01, T02…), cada una con un responsable del equipo, su fase,
  dependencias (blocked_by) sin ciclos y un criterio de terminado.
- Puntos de control al acabar requisitos y arquitectura con human_approval=true (el humano aprueba).
  Después de implementación y QA, condiciones comprobables (pruebas en verde, UAT superado).

## Proyecto
- Respeta lo que pida el usuario (tecnologías, límites, idioma). Si algo no está claro, decide algo
  razonable, anótalo en assumptions y pon la duda en open_questions.
- Por defecto: todo en local, sin servicios de pago ni credenciales reales.
- Si la carpeta del proyecto ya tiene contenido, revísalo con Read/Glob/Grep y planifica a partir de él.
"""


@dataclass
class ArchitectResult:
    plan: dict
    cost_usd: float | None
    duration_ms: int | None
    session_id: str | None


class ArchitectError(RuntimeError):
    pass


def build_command(prompt: str, models: tuple[str, ...], model: str, effort: str) -> list[str]:
    fable_note = " (fable: el de más capacidad; úsalo solo si está permitido y lo justificas)" \
        if "fable" in models else ""
    system = SYSTEM_PROMPT.format(models=", ".join(models), fable_note=fable_note)
    return [
        "claude", "-p",
        "--model", model,
        "--effort", effort,
        "--output-format", "json",
        "--json-schema", json.dumps(P.schema(models)),
        "--append-system-prompt", system,
        "--allowedTools", "Read", "Glob", "Grep",
        "--disallowedTools", "Write", "Edit", "Bash", "NotebookEdit", "WebFetch", "WebSearch",
        "--", prompt,
    ]


def run(prompt: str, directory: Path, models: tuple[str, ...] = P.DEFAULT_MODELS,
        model: str = "opus", effort: str = "high", timeout: int = 900) -> ArchitectResult:
    if not shutil.which("claude"):
        raise ArchitectError("No encuentro el comando `claude` en el PATH.")
    directory.mkdir(parents=True, exist_ok=True)
    cmd = build_command(prompt, models, model, effort)
    try:
        out = subprocess.run(cmd, cwd=directory, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        raise ArchitectError(f"El arquitecto no ha terminado en {timeout} s.") from exc
    try:
        data = json.loads(out.stdout)
    except ValueError as exc:
        detail = (out.stderr or out.stdout).strip()[-400:]
        raise ArchitectError(f"Respuesta no válida de claude (código {out.returncode}): {detail}") from exc
    if data.get("is_error") or data.get("subtype") != "success":
        raise ArchitectError(f"claude devolvió un error: {str(data.get('result'))[:400]}")
    result = data.get("structured_output")
    if result is None:   # por si una versión antigua solo deja el texto
        try:
            result = json.loads(data.get("result") or "")
        except ValueError as exc:
            raise ArchitectError("La respuesta no trae el JSON del plan.") from exc
    if errors := P.validate(result, models):
        raise ArchitectError("El plan no es coherente:\n- " + "\n- ".join(errors))
    return ArchitectResult(result, data.get("total_cost_usd"), data.get("duration_ms"),
                           data.get("session_id"))
