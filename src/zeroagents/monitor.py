"""Junta los agentes de todas las plataformas en una sola vista."""

from __future__ import annotations

from zeroagents.adapters import ADAPTERS
from zeroagents.model import Snapshot


class Monitor:
    def __init__(self) -> None:
        self.adapters = [cls() for cls in ADAPTERS.values()]

    def snapshot(self, include_ended: bool = False) -> Snapshot:
        agents, limits = [], {}
        for adapter in self.adapters:
            found, lim = adapter.collect(include_ended)
            agents.extend(found)
            if lim:
                limits[adapter.platform] = lim
        # Primero lo que te necesita; dentro de cada estado, lo más reciente.
        agents.sort(key=lambda a: (a.status.rank, -(a.last_activity or 0)))
        return Snapshot(agents=agents, limits=limits)
