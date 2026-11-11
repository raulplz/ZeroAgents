"""Modo Work: de un prompt de proyecto a un equipo de agentes listo para trabajar.

Flujo:
1. `architect`: un agente «arquitecto de equipos» lee tu prompt y devuelve, en
   JSON validado, el planteamiento del proyecto, el equipo (cada agente con su
   modelo y su esfuerzo), las tareas con dependencias y los puntos de control.
2. `scaffold`: convierte ese plan en archivos del proyecto (`.claude/agents/*.md`,
   `docs/PLAN.md`, `docs/EQUIPO.md`, `.zeroagents/`). Nunca sobrescribe nada.
3. `launch`: abre el agente líder en una terminal con los equipos de Claude Code
   activados solo para ese proceso.
4. `live`: lee el estado del equipo y sus tareas para el panel.
"""
