"""Almacenamiento sin bloqueos.

- Los eventos se añaden como líneas a `<sesión>.events.jsonl`. Una escritura
  pequeña en modo append es atómica, así que varios hooks asíncronos pueden
  escribir a la vez sin pisarse.
- Las instantáneas (p. ej. la statusline) se reemplazan con `os.replace`: gana
  la última, que es justo lo que queremos.
"""

from __future__ import annotations

import json
import os
import time
from pathlib import Path


def append_event(path: Path, event: dict) -> None:
    line = json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n"
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
    try:
        os.write(fd, line.encode())
    finally:
        os.close(fd)


def write_snapshot(path: Path, data: dict) -> None:
    tmp = path.with_suffix(f".{os.getpid()}.tmp")
    tmp.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    os.replace(tmp, path)


def read_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


class JsonlTail:
    """Lee un .jsonl de forma incremental: cada llamada devuelve solo lo nuevo."""

    def __init__(self, path: Path):
        self.path = path
        self.offset = 0

    def read_new(self) -> list[dict]:
        try:
            size = self.path.stat().st_size
        except OSError:
            return []
        if size < self.offset:   # el archivo se ha truncado o reemplazado
            self.offset = 0
        if size == self.offset:
            return []
        with self.path.open("rb") as fh:
            fh.seek(self.offset)
            chunk = fh.read(size - self.offset)
        # Deja para la próxima vez una línea que aún se esté escribiendo.
        end = chunk.rfind(b"\n") + 1
        self.offset += end
        out = []
        for raw in chunk[:end].splitlines():
            try:
                out.append(json.loads(raw))
            except ValueError:
                continue
        return out


def prune(directory: Path, max_age_days: float = 7) -> None:
    """Borra estado de ZeroAgents (nunca datos de Claude) con más de N días."""
    limit = time.time() - max_age_days * 86400
    for path in directory.glob("*"):
        try:
            if path.is_file() and path.stat().st_mtime < limit:
                path.unlink()
        except OSError:
            pass
