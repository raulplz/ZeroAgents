from __future__ import annotations

from abc import ABC, abstractmethod

from zeroagents.model import Agent, Limits


class Adapter(ABC):
    platform: str
    label: str

    @abstractmethod
    def collect(self, include_ended: bool = False) -> tuple[list[Agent], Limits | None]:
        """Devuelve los agentes de la plataforma y sus límites de uso."""
