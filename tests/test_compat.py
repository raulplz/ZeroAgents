"""Compatibilidad TUI (Python) ↔ núcleo de la GUI (Rust): mismos archivos, sin pérdidas.

Usa el ejemplo `compat_helper` del núcleo de Rust. Si no está compilado se omite:
    cd gui && cargo build -p zeroagents-core --example compat_helper
La parte contraria (Rust abre lo que escribe Python) está en `gui/core/tests/compat.rs`.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).parent))

import compat_helper  # noqa: E402
from zeroagents.work import live  # noqa: E402


def _helper() -> Path | None:
    for profile in ("debug", "release"):
        path = ROOT / "gui" / "target" / profile / "examples" / "compat_helper"
        if path.is_file():
            return path
    return None


pytestmark = pytest.mark.skipif(_helper() is None, reason="falta el ejemplo compat_helper de Rust")


@pytest.fixture
def env(tmp_path, monkeypatch):
    (tmp_path / "home").mkdir()
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    monkeypatch.setenv("ZEROAGENTS_STATE_DIR", str(tmp_path / "state"))
    monkeypatch.setenv("CLAUDE_CONFIG_DIR", str(tmp_path / "claude"))
    return tmp_path


def rust(*args: str, **kw) -> subprocess.Popen | str:
    cmd = [str(_helper()), *args]
    if kw.get("background"):
        return subprocess.Popen(cmd)
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def plan(directory: Path) -> Path:
    (directory / ".zeroagents").mkdir(parents=True)
    (directory / ".zeroagents" / "team.json").write_text("{}")
    return directory


def test_rust_escribe_y_python_lee_el_registro(env):
    a, b = plan(env / "a"), plan(env / "b")
    rust("register", str(a), "A")
    live.register(b, "B")
    rust("register", str(a), "A otra vez")           # sube arriba, sin duplicar
    assert live.projects() == [a, b]
    assert json.loads(rust("projects")) == [str(a), str(b)]


def test_python_conserva_lo_que_rust_no_conoce_y_viceversa(env):
    path = env / "state" / "work" / "projects.json"
    path.parent.mkdir(parents=True)
    path.write_text(json.dumps({"version": 9, "extra": {"k": 1},
                                "projects": [{"dir": "/p", "name": "P", "added": 1, "etiqueta": "x"}]}))
    live.register(Path("/p"), "P2")
    rust("register", "/q", "Q")
    data = json.loads(path.read_text())
    assert data["version"] == 9 and data["extra"] == {"k": 1}
    p = next(i for i in data["projects"] if i["dir"] == "/p")
    assert p["etiqueta"] == "x" and p["name"] == "P2"


def test_python_lee_lo_que_escribe_rust_y_al_reves_la_sesion(env):
    compat_helper.ingest()
    seen = json.loads(rust("collect"))
    assert seen == compat_helper.collect()


def test_escrituras_simultaneas_python_y_rust(env):
    per = 25
    procs = [rust("register", str(env / f"rs{i}"), f"rs{i}", str(per), background=True) for i in range(3)]
    errors: list[Exception] = []

    def writer(i: int) -> None:
        try:
            for n in range(per):
                live.register(env / f"py{i}-{n}", f"py{i}-{n}")
        except Exception as exc:  # pragma: no cover
            errors.append(exc)

    threads = [threading.Thread(target=writer, args=(i,)) for i in range(3)]
    path = env / "state" / "work" / "projects.json"
    stop = threading.Event()

    def reader() -> None:
        while not stop.is_set():
            if path.exists():
                try:
                    json.loads(path.read_text())
                except ValueError as exc:  # pragma: no cover
                    errors.append(exc)

    watcher = threading.Thread(target=reader)
    watcher.start()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert all(p.wait() == 0 for p in procs)
    stop.set()
    watcher.join()
    assert not errors
    dirs = [p["dir"] for p in json.loads(path.read_text())["projects"]]
    assert len(dirs) == len(set(dirs)) == 6 * per


def test_alta_desde_la_gui_la_ve_python(env):
    proj = env / "proyecto"
    (proj / ".zeroagents").mkdir(parents=True)
    (proj / ".zeroagents" / "team.json").write_text(json.dumps({"plan": {"project": {"name": "Del plan"}}}))
    assert rust("register-dir", str(proj)).strip() == "Del plan"
    assert live.projects() == [proj.resolve()]
    assert rust("register-dir", str(proj), "Renombrado").strip() == "Renombrado"
    data = json.loads((env / "state" / "work" / "projects.json").read_text())
    assert [p["name"] for p in data["projects"]] == ["Renombrado"]
    sin_plan = env / "vacio"
    sin_plan.mkdir()
    assert subprocess.run([str(_helper()), "register-dir", str(sin_plan)], capture_output=True).returncode != 0
