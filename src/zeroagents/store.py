"""Almacenamiento compartido con la GUI de Rust (ver docs/datos.md).

- Los eventos se añaden como líneas a `<sesión>.events.jsonl`. Una escritura
  pequeña en modo append es atómica, así que varios hooks asíncronos pueden
  escribir a la vez sin pisarse.
- Las instantáneas (p. ej. la statusline) se reemplazan con `os.replace`: gana
  la última, que es justo lo que queremos.
- Los archivos que se modifican leyendo y reescribiendo (registro de proyectos,
  libro de subagentes) usan `update_json`: bloqueo `flock` sobre `<archivo>.lock`
  (el mismo que usa la GUI), lectura fresca dentro del bloqueo, conservación de
  los campos desconocidos y escritura atómica (tmp + fsync + rename).
"""

from __future__ import annotations

import contextlib
import fcntl
import json
import os
import shutil
import time
from collections.abc import Callable, Iterator
from pathlib import Path


def append_event(path: Path, event: dict) -> None:
    line = json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n"
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
    try:
        os.write(fd, line.encode())
    finally:
        os.close(fd)


def write_atomic(path: Path, text: str) -> None:
    """Escribe en un temporal del mismo directorio y lo renombra encima: un lector
    ve siempre el contenido anterior completo o el nuevo completo."""
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with open(tmp, "w", encoding="utf-8") as fh:
            fh.write(text)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp, path)
    finally:
        with contextlib.suppress(OSError):
            tmp.unlink()


def write_snapshot(path: Path, data: dict) -> None:
    write_atomic(path, json.dumps(data, ensure_ascii=False))


@contextlib.contextmanager
def locked(path: Path) -> Iterator[None]:
    """Bloqueo exclusivo entre procesos (Python y Rust) sobre `<path>.lock`."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path.with_name(path.name + ".lock"), os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield
    finally:
        os.close(fd)  # cerrar libera el bloqueo


def update_json(path: Path, change: Callable[[dict], None], *,
                migrate: Callable[[dict], bool] | None = None, indent: int | None = None) -> dict:
    """Lee, modifica y reescribe un JSON bajo bloqueo, sin perder nada.

    - `change` recibe el dict leído (con todos sus campos, también los que este
      código no conoce) y lo modifica en sitio.
    - `migrate`, si se pasa, adapta un archivo antiguo antes del cambio y devuelve
      True si lo tocó; en ese caso se guarda antes `<archivo>.bak` con el original.
    - Si el archivo existe pero no es JSON válido, se aparta como
      `<archivo>.corrupto-<fecha>` en lugar de pisarlo en silencio.
    """
    with locked(path):
        data: dict = {}
        if path.exists():
            try:
                loaded = json.loads(path.read_text(encoding="utf-8"))
                if not isinstance(loaded, dict):
                    raise ValueError("no es un objeto")
                data = loaded
            except (OSError, ValueError):
                shutil.copy2(path, path.with_name(f"{path.name}.corrupto-{time.strftime('%Y%m%d-%H%M%S')}"))
        if migrate and data and migrate(data):
            shutil.copy2(path, path.with_name(path.name + ".bak"))
        elif migrate and not data:
            migrate(data)
        change(data)
        text = json.dumps(data, ensure_ascii=False, indent=indent)
        write_atomic(path, text + ("\n" if indent else ""))
        return data


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
