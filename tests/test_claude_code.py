import json

import pytest

from zeroagents.adapters import claude_code as cc
from zeroagents.model import Status


def ev(e, t, **kw):
    return {"e": e, "t": t, **kw}


def test_permission_request_needs_you_until_tool_runs():
    state = cc.fold([
        ev("UserPromptSubmit", 1, pm="default"),
        ev("PermissionRequest", 2, tool="Bash", msg="Bash: borrar build"),
    ])
    status, need, _ = state.status()
    assert status is Status.PERMISSION and need == "Bash: borrar build"
    cc.fold([ev("PostToolUse", 3, tool="Bash")], state)
    assert state.status()[0] is Status.WORKING


def test_subagent_permission_not_cleared_by_main_agent_activity():
    state = cc.fold([
        ev("SubagentStart", 1, ag="a1"),
        ev("PermissionRequest", 2, ag="a1", msg="Write: x.py"),
        ev("PreToolUse", 3, tool="Read"),
    ])
    assert state.status()[0] is Status.PERMISSION
    assert len(state.subagents) == 1
    cc.fold([ev("PostToolUse", 4, ag="a1"), ev("SubagentStop", 5, ag="a1")], state)
    assert state.status()[0] is Status.WORKING and not state.subagents


def test_question_tools_and_stop():
    state = cc.fold([
        ev("UserPromptSubmit", 1),
        ev("PreToolUse", 2, tool="AskUserQuestion", msg="¿Qué base de datos?"),
    ])
    assert state.status()[:2] == (Status.QUESTION, "¿Qué base de datos?")
    cc.fold([ev("PostToolUse", 3, tool="AskUserQuestion"), ev("Stop", 4, last="Hecho.")], state)
    assert state.status()[0] is Status.YOUR_TURN
    assert state.last_message == "Hecho."


def test_permission_notification_and_idle():
    state = cc.fold([ev("UserPromptSubmit", 1), ev("Notification", 8, nt="permission_prompt", msg="x")])
    assert state.status()[0] is Status.PERMISSION
    cc.fold([ev("PostToolUse", 9)], state)
    assert state.status()[0] is Status.WORKING
    cc.fold([ev("Stop", 10), ev("Notification", 70, nt="idle_prompt", msg="esperando")], state)
    assert state.status()[0] is Status.YOUR_TURN


def test_mode_and_effort_keep_latest_value():
    state = cc.fold([ev("UserPromptSubmit", 5, pm="plan"), ev("PostToolUse", 2, pm="default", eff="high")])
    assert state.value("mode")[0] == "plan"
    assert state.value("effort")[0] == "high"


def test_event_from_hook_describes_permission():
    data = {
        "session_id": "s", "hook_event_name": "PermissionRequest", "permission_mode": "default",
        "tool_name": "Bash", "tool_input": {"command": "rm -rf build", "description": "Borrar build"},
        "effort": {"level": "high"},
    }
    out = cc.event_from_hook(data, now=1.0)
    assert out == {"t": 1.0, "e": "PermissionRequest", "pm": "default", "eff": "high",
                   "tool": "Bash", "msg": "Bash: Borrar build"}


def test_transcript_deduplicates_split_responses():
    stats = cc.TranscriptStats()
    usage = {"input_tokens": 2, "cache_creation_input_tokens": 10, "cache_read_input_tokens": 100,
             "output_tokens": 5}
    line = {"type": "assistant", "requestId": "r1", "timestamp": "2026-10-26T20:00:00Z",
            "effort": "high", "message": {"model": "claude-opus-5-5", "usage": usage}}
    stats.apply(line)
    stats.apply(line)  # misma respuesta en dos líneas
    stats.apply({"type": "user", "permissionMode": "auto", "timestamp": "2026-10-26T20:01:00Z"})
    assert (stats.requests, stats.input_tokens, stats.cache_read_tokens, stats.output_tokens) == (1, 12, 100, 5)
    assert stats.values["model"][0] == "claude-opus-5-5"
    assert stats.values["mode"][0] == "auto"


@pytest.mark.parametrize("raw, pretty", [
    ("claude-opus-5-5", "Opus 5.5"),
    ("claude-sonnet-5-5[1m]", "Sonnet 5.5 · 1M"),
    ("claude-haiku-4-5-20251001", "Haiku 4.5"),
    ("claude-fable-5-1", "Fable 5.1"),
    (None, None),
])
def test_pretty_model(raw, pretty):
    assert cc.pretty_model(raw) == pretty


def test_registry_idle_clears_stale_permission(tmp_path, monkeypatch):
    """Pulsar Esc en un permiso no dispara hooks: el registro «idle» lo resuelve."""
    monkeypatch.setenv("ZEROAGENTS_STATE_DIR", str(tmp_path / "state"))
    monkeypatch.setenv("CLAUDE_CONFIG_DIR", str(tmp_path / "claude"))
    adapter = cc.ClaudeCodeAdapter()
    events = adapter.dir / "s1.events.jsonl"
    events.write_text(json.dumps(ev("PermissionRequest", 100.0, msg="Bash: ls")) + "\n")

    busy = {"sessionId": "s1", "status": "busy", "statusUpdatedAt": 100_500, "cwd": "/x"}
    assert adapter._build("s1", busy)[0].status is Status.PERMISSION
    idle = {**busy, "status": "idle", "statusUpdatedAt": 110_000}
    assert adapter._build("s1", idle)[0].status is Status.YOUR_TURN
