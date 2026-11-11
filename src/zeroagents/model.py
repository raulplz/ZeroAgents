"""Modelo común a todas las plataformas.

Cada adaptador (Claude Code, y en el futuro Codex u otros) traduce sus datos
a `Agent`, así el panel no necesita saber de dónde viene cada agente.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum


class Status(Enum):
    """Estado del agente, ordenado de más a menos urgente para ti."""

    PERMISSION = "permiso"      # espera que apruebes una herramienta
    QUESTION = "pregunta"       # te ha hecho una pregunta o espera un plan
    WORKING = "trabajando"
    YOUR_TURN = "tu turno"      # terminó y espera tu siguiente mensaje
    UNKNOWN = "desconocido"
    ENDED = "cerrada"

    @property
    def needs_you(self) -> bool:
        return self in (Status.PERMISSION, Status.QUESTION)

    @property
    def rank(self) -> int:
        return list(Status).index(self)


@dataclass
class Usage:
    cost_usd: float | None = None          # estimación a precio de lista
    context_pct: float | None = None
    context_size: int | None = None
    input_tokens: int = 0                  # nuevos + escritos en caché
    output_tokens: int = 0
    cache_read_tokens: int = 0             # contexto releído; mucho más barato
    requests: int = 0
    duration_ms: int | None = None


@dataclass
class Agent:
    platform: str
    id: str
    name: str
    cwd: str = ""
    status: Status = Status.UNKNOWN
    model: str | None = None
    effort: str | None = None
    mode: str | None = None
    fast: bool = False
    usage: Usage = field(default_factory=Usage)
    attention_msg: str | None = None       # qué necesita de ti
    last_message: str | None = None        # última respuesta del agente
    last_activity: float | None = None     # epoch en segundos
    subagents: int = 0
    pid: int | None = None
    kind: str | None = None
    alive: bool = True
    transcript: str | None = None


@dataclass
class Limits:
    """Límites de uso de la cuenta (comunes a todas las sesiones)."""

    five_hour_pct: float | None = None
    five_hour_resets: float | None = None
    seven_day_pct: float | None = None
    seven_day_resets: float | None = None
    observed_at: float | None = None


@dataclass
class Snapshot:
    agents: list[Agent]
    limits: dict[str, Limits]              # por plataforma
