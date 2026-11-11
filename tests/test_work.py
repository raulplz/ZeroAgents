import json
from pathlib import Path

import pytest

from zeroagents.model import Agent, Status
from zeroagents.work import architect, launch, live, scaffold
from zeroagents.work import plan as P


def make_plan(**over):
    agent = lambda name, phase, model="sonnet", effort="medium", outputs=(): {  # noqa: E731
        "name": name, "title": name.replace("-", " ").title(), "phase": phase, "model": model,
        "effort": effort, "why": "porque sí", "description": f"Agente {name}",
        "tools": ["Read", "Write"], "inputs": ["docs/requisitos.md"], "outputs": list(outputs),
        "system_prompt": f"Eres {name}.",
    }
    plan = {
        "project": {"name": "Poco Plan", "slug": "poco-plan", "summary": "App de planes.", "goal": "MVP",
                    "users": ["organizador"], "stack": ["Python"], "in_scope": ["planes"],
                    "out_of_scope": ["pagos"], "assumptions": ["local"], "open_questions": ["¿Login?"]},
        "lead": {"model": "opus", "effort": "high", "why": "coordina"},
        "agents": [agent("analista", "requisitos", "opus", "high", ["docs/requisitos.md"]),
                   agent("dev-backend", "implementacion", outputs=["src/"]),
                   agent("qa", "qa", "haiku", "low", ["qa/informe.md"])],
        "tasks": [
            {"id": "T01", "subject": "Requisitos", "description": "", "owner": "analista",
             "phase": "requisitos", "blocked_by": [], "done_when": "RF con criterios"},
            {"id": "T02", "subject": "API", "description": "", "owner": "dev-backend",
             "phase": "implementacion", "blocked_by": ["T01"], "done_when": "tests verdes"},
            {"id": "T03", "subject": "Pruebas", "description": "", "owner": "qa",
             "phase": "qa", "blocked_by": ["T02"], "done_when": "E2E ok"},
        ],
        "gates": [{"after_phase": "requisitos", "condition": "RF aprobados", "human_approval": True}],
    }
    plan.update(over)
    return plan


@pytest.fixture
def dirs(tmp_path, monkeypatch):
    monkeypatch.setenv("ZEROAGENTS_STATE_DIR", str(tmp_path / "state"))
    monkeypatch.setenv("CLAUDE_CONFIG_DIR", str(tmp_path / "claude"))
    project = tmp_path / "proyecto"
    project.mkdir()
    return tmp_path, project


def test_schema_requires_everything_and_limits_models():
    s = P.schema(("haiku", "sonnet"))
    assert set(s["required"]) == {"project", "lead", "agents", "tasks", "gates"}
    agent = s["properties"]["agents"]["items"]
    assert agent["properties"]["model"]["enum"] == ["haiku", "sonnet"]
    assert agent["additionalProperties"] is False


def test_validate_ok_and_errors():
    assert P.validate(make_plan()) == []
    bad = make_plan()
    bad["tasks"][1]["owner"] = "fantasma"
    bad["tasks"][0]["blocked_by"] = ["T03"]          # ciclo T01→T03→T02→T01
    bad["agents"][0]["name"] = "team-lead"
    errors = " | ".join(P.validate(bad))
    assert "fantasma" in errors and "ciclo" in errors and "reservado" in errors


def test_validate_rejects_disallowed_model():
    plan = make_plan()
    plan["agents"][0]["model"] = "fable"
    assert any("no permitido" in e for e in P.validate(plan, ("haiku", "sonnet", "opus")))


def test_scaffold_writes_agents_with_model_and_effort(dirs):
    _, project = dirs
    report = scaffold.write(project, make_plan(), {"prompt": "x"})
    agent_md = (project / ".claude/agents/analista.md").read_text()
    front = agent_md.split("---")[1]
    assert "model: opus" in front and "effort: high" in front and "name: analista" in front
    assert "Eres analista." in agent_md and "docs/EQUIPO.md" in agent_md
    rows = [line for line in (project / "docs/PLAN.md").read_text().splitlines() if line.startswith("| T0")]
    assert len(rows) == 3
    lead = (project / scaffold.LEAD_PROMPT_FILE).read_text()
    assert "`dev-backend`" in lead and "T02 · API → dev-backend (depende de T01)" in lead
    assert "**Product Owner**" in lead and "Aceptada cuando: tests verdes" in lead
    assert "Product Owner" in (project / "docs/EQUIPO.md").read_text()
    assert P.load(project)["plan"]["project"]["name"] == "Poco Plan"
    assert len(report.written) >= 7


def test_scaffold_never_overwrites(dirs):
    _, project = dirs
    (project / "docs").mkdir()
    (project / "docs/PLAN.md").write_text("mío")
    scaffold.write(project, make_plan(), {})
    report = scaffold.write(project, make_plan(), {})
    assert (project / "docs/PLAN.md").read_text() == "mío"
    assert any(p.name.startswith("team.") and p.name != "team.json" for p in report.skipped)


def test_architect_command_is_read_only_and_structured():
    cmd = architect.build_command("haz una app", ("haiku", "sonnet"), "opus", "high")
    assert cmd[:2] == ["claude", "-p"] and "--json-schema" in cmd
    assert "Write" in cmd[cmd.index("--disallowedTools"):] and cmd[-1] == "haz una app"
    assert json.loads(cmd[cmd.index("--json-schema") + 1])["type"] == "object"


def test_lead_command_uses_plan_model(dirs):
    _, project = dirs
    scaffold.write(project, make_plan(), {})
    cmd = launch.lead_command(P.load(project))
    assert cmd[:5] == ["claude", "--model", "opus", "--effort", "high"]


def test_live_without_team_uses_plan(dirs):
    _, project = dirs
    scaffold.write(project, make_plan(), {})
    live.register(project, "Poco Plan")
    assert live.projects() == [project]
    view = live.build(project, [])
    assert view.progress == (0, 3) and not view.live_tasks
    assert [t.blocked for t in view.tasks] == [False, True, True]
    assert view.members[0].state == "sin arrancar"
    assert view.phase == "1 · Requisitos"


def test_live_reads_real_team_leniently(dirs):
    tmp, project = dirs
    scaffold.write(project, make_plan(), {})
    sid = "abcdef12-0000-0000-0000-000000000000"
    team = tmp / "claude/teams/session-abcdef12"
    team.mkdir(parents=True)
    (team / "config.json").write_text(json.dumps({"members": [{"name": "team-lead"}, {"name": "analista"},
                                                              {"name": "dev-backend"}]}))
    tasks = tmp / "claude/tasks/session-abcdef12"
    tasks.mkdir(parents=True)
    (tasks / "1.json").write_text(json.dumps({"id": "1", "subject": "Requisitos", "status": "completed",
                                              "owner": "analista"}))
    (tasks / "2.json").write_text(json.dumps({"id": "2", "subject": "API", "status": "in_progress",
                                              "owner": "dev-backend", "blockedBy": ["1"]}))
    (tasks / "3.json").write_text(json.dumps({"id": "3", "subject": "Pruebas", "status": "pending",
                                              "owner": "qa", "blockedBy": ["2"]}))
    lead = Agent(platform="claude-code", id=sid, name="lead", cwd=str(project), status=Status.WORKING)
    view = live.build(project, [lead])
    assert view.team_name == "session-abcdef12" and view.live_tasks
    assert view.progress == (1, 3)
    states = {m.name: (m.state, m.current, m.in_team) for m in view.members}
    assert states["dev-backend"] == ("trabajando", "API", True)
    assert states["analista"][0] == "terminado"
    assert states["qa"][0] == "esperando dependencias" and not states["qa"][2]
    assert view.phase == "3 · Implementación"


def test_launch_uses_permission_mode_chosen_at_creation(dirs, monkeypatch):
    _, project = dirs
    scaffold.write(project, make_plan(), {"launch": {"permission_mode": "acceptEdits"}})
    calls = []
    monkeypatch.setattr(launch.subprocess, "Popen", lambda cmd, **kw: calls.append(cmd))
    used = launch.launch(project)
    assert "--permission-mode acceptEdits" in used and calls
    scaffold.write(project, make_plan(), {"launch": {"permission_mode": "default"}})
    assert "--permission-mode" not in launch.launch(project)
