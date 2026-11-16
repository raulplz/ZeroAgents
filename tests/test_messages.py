import json

import pytest

from zeroagents import messages as M


@pytest.fixture
def state(tmp_path, monkeypatch):
    monkeypatch.setenv("ZEROAGENTS_STATE_DIR", str(tmp_path / "state"))
    monkeypatch.delenv("CLAUDE_PROJECT_DIR", raising=False)
    return tmp_path


def pre_agent(sid="s1", **ti):
    return {"session_id": sid, "hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_use_id": "tu1",
            "cwd": "/nada", "tool_input": {"subagent_type": "analista-requisitos", "description": "Requisitos V1",
                                            "prompt": "ENCARGO · analista-requisitos · Requisitos V1", **ti}}


def post_agent(status="completed", aid="a1", text="ENTREGA · analista-requisitos"):
    resp = {"status": status, "agentId": aid}
    if status == "completed":
        resp["content"] = [{"type": "text", "text": text}]
    return {"session_id": "s1", "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "tu1",
            "cwd": "/nada", "tool_input": {"subagent_type": "analista-requisitos", "description": "Requisitos V1"},
            "tool_response": resp}


def saved(state):
    path = state / "state" / "claude-code" / "s1.messages.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def test_foreground_encargo_and_entrega(state):
    M.ingest(pre_agent())
    M.ingest(post_agent())
    msgs = saved(state)
    assert [m["kind"] for m in msgs] == ["encargo", "entrega"]
    assert msgs[0]["de"] == "principal" and msgs[0]["para"] == "analista-requisitos"
    assert msgs[0]["texto"].startswith("ENCARGO")
    assert msgs[1]["de"] == "analista-requisitos" and msgs[1]["texto"] == "ENTREGA · analista-requisitos"


def test_background_entrega_comes_from_subagent_stop(state):
    M.ingest(pre_agent())
    M.ingest(post_agent(status="async_launched", aid="bg1"))
    assert len(saved(state)) == 1   # el lanzamiento no es una entrega
    M.ingest({"session_id": "s1", "hook_event_name": "SubagentStop", "agent_id": "bg1",
              "agent_type": "analista-requisitos", "last_assistant_message": "Hecho en segundo plano"})
    msgs = saved(state)
    assert msgs[-1]["kind"] == "entrega" and msgs[-1]["texto"] == "Hecho en segundo plano"
    assert msgs[-1]["via"] == "segundo plano"


def test_internal_agents_are_ignored(state):
    M.ingest({"session_id": "s1", "hook_event_name": "SubagentStop", "agent_id": "interno",
              "agent_type": "", "last_assistant_message": "sugerencia de prompt"})
    assert saved(state) == []


def test_handback_replaces_short_note(state):
    M.ingest(pre_agent())
    M.ingest({"session_id": "s1", "hook_event_name": "PreToolUse", "tool_name": "SubagentHandback",
              "agent_id": "a1", "agent_type": "analista-requisitos", "tool_input": {"message": "Informe completo"}})
    M.ingest(post_agent(aid="a1", text="(nota: informe entregado por handback)"))
    msgs = saved(state)
    assert [m["kind"] for m in msgs] == ["encargo", "entrega"]
    assert msgs[1]["texto"] == "Informe completo"


def test_send_message_between_teammates(state):
    M.ingest({"session_id": "s1", "hook_event_name": "PreToolUse", "tool_name": "SendMessage",
              "agent_type": "dev-backend", "tool_input": {"to": "qa-pruebas", "message": "API lista", "summary": "API"}})
    m = saved(state)[0]
    assert (m["kind"], m["de"], m["para"], m["texto"]) == ("mensaje", "dev-backend", "qa-pruebas", "API lista")


def test_other_tools_are_not_messages(state):
    M.ingest({"session_id": "s1", "hook_event_name": "PreToolUse", "tool_name": "Bash",
              "tool_input": {"command": "ls"}})
    assert saved(state) == []


def test_project_log_only_when_folder_exists(state, monkeypatch):
    project = state / "proyecto"
    project.mkdir()
    monkeypatch.setenv("CLAUDE_PROJECT_DIR", str(project))
    M.ingest(pre_agent())
    assert not (project / ".zeroagents").exists()
    (project / ".zeroagents").mkdir()
    M.ingest(pre_agent())
    log = project / ".zeroagents" / "mensajes.jsonl"
    assert len(log.read_text().splitlines()) == 1


def test_long_text_is_cut(state):
    M.ingest(pre_agent(prompt="x" * (M.MAX_TEXT + 10)))
    assert saved(state)[0]["texto"].endswith("[… recortado por ZeroAgents]")


def test_markdown_keeps_full_text():
    md = M.to_markdown([{"t": 0, "kind": "encargo", "de": "principal", "para": "qa", "titulo": "T",
                         "texto": "línea 1\nlínea 2"}])
    assert "## 1. ENCARGO · principal → qa · T" in md and "línea 1\nlínea 2" in md


def test_format_clips_long_bodies():
    text = "\n".join(f"l{i}" for i in range(20))
    out = M.format_message({"t": 0, "kind": "entrega", "de": "a", "para": "b", "texto": text}, color=False)
    assert "14 líneas más" in out and "l19" not in out
    assert "l19" in M.format_message({"t": 0, "kind": "entrega", "de": "a", "para": "b", "texto": text},
                                     full=True, color=False)
