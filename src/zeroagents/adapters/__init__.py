"""Adaptadores por plataforma. Para añadir una (p. ej. Codex), crea un módulo
con una clase que implemente `Adapter` y regístrala en `ADAPTERS`."""

from __future__ import annotations

from zeroagents.adapters.base import Adapter
from zeroagents.adapters.claude_code import ClaudeCodeAdapter

ADAPTERS: dict[str, type[Adapter]] = {
    ClaudeCodeAdapter.platform: ClaudeCodeAdapter,
}
