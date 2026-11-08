"""AppHarness tests: the real app process speaking the PGAP protocol."""

import json
from pathlib import Path

import pytest

from plexi_sdk.testing import AppHarness

from app import model

APP = Path(__file__).resolve().parent.parent / "main.py"

TASKS = [
    {"id": "0001", "title": "Alpha thing", "priority": "p1", "state": "ready", "size": "s", "blocked": False, "blockers": []},
    {"id": "0002", "title": "Beta thing", "priority": "p2", "state": "ready", "size": "m", "blocked": False, "blockers": []},
    {"id": "0003", "title": "Gamma", "priority": None, "state": "proposed", "size": "", "blocked": False, "blockers": []},
]
FORBIDDEN = ("executable", "argv", "env", "cwd", "command", "shell", "workspace")


def ops(cmds):
    return [e for e in cmds if e.get("type") == "cli_operation"]


def result(h, request_id, operation_id, **kw):
    h._send({"type": "cli_operation_result", "request_id": request_id, "operation_id": operation_id,
             "ok": kw.pop("ok", True), **kw})


def boot(h, first=None):
    """Drive startup: version, list, show."""
    first = first if first is not None else ops(h.run(1))
    assert [o["operation_id"] for o in first] == [model.OP_VERSION]
    result(h, first[0]["request_id"], model.OP_VERSION, data=f"stint {model.EXPECTED_STINT}\n")
    nxt = ops(h.run(1))
    assert [o["operation_id"] for o in nxt] == [model.OP_LIST]
    result(h, nxt[0]["request_id"], model.OP_LIST, data=TASKS)
    show = ops(h.run(1))
    assert show and show[0]["operation_id"] == model.OP_SHOW and show[0]["values"] == {"task": "0001"}
    result(h, show[0]["request_id"], model.OP_SHOW, data={"id": "0001", "body": "## Why\nBecause", "area": ["cli"]})
    return h.run(1)


@pytest.mark.parametrize("size", [(300, 200), (800, 600), (1200, 800)])
def test_renders_without_overlap_before_and_after_load(size):
    w, hh = size
    with AppHarness(str(APP), width=w, height=hh) as h:
        first = ops(h.run(1))
        h.assert_no_overlap()
        boot(h, first)
        h.assert_no_overlap()


def test_startup_requests_only_declared_operations_with_no_forbidden_fields():
    with AppHarness(str(APP)) as h:
        sent = ops(h.run(1))
    assert len(sent) == 1
    assert sent[0]["operation_id"] == "stint.version"
    assert not any(name in sent[0] for name in FORBIDDEN)


def test_board_shows_tasks_columns_and_detail():
    with AppHarness(str(APP)) as h:
        shown = json.dumps(boot(h))
    for needle in ("P1 (1)", "P2 (1)", "unset (1)", "Alpha thing", "Because"):
        assert needle in shown, needle


def test_keyboard_priority_change_sends_the_guarded_set_and_waits_for_stint():
    with AppHarness(str(APP)) as h:
        boot(h)
        h.key("0")
        sent = ops(h.run(1))
        assert len(sent) == 1 and sent[0]["operation_id"] == "stint.set_priority"
        assert sent[0]["values"] == {"priority": "p0", "expected": "p1", "task": "0001"}
        assert "Moving 0001 p1 to p0" in json.dumps(h.run(1))
        result(h, sent[0]["request_id"], model.OP_SET, data={"updated": "0001", "priority": "p0"})
        assert "moved p1 to p0" in json.dumps(h.run(1))


def test_mouse_priority_button_and_row_click_use_the_same_path():
    with AppHarness(str(APP)) as h:
        boot(h)
        h.ui_action("pick-p2:0")
        sent = ops(h.run(1))
        assert sent and sent[0]["operation_id"] == "stint.show" and sent[0]["values"] == {"task": "0002"}
        result(h, sent[0]["request_id"], model.OP_SHOW, data={"id": "0002", "body": "b"})
        h.run(1)
        h.ui_action("prio:p0")
        sent = ops(h.run(1))
        assert sent[0]["values"] == {"priority": "p0", "expected": "p2", "task": "0002"}


def test_conflict_is_explained_and_retry_is_explicit():
    with AppHarness(str(APP)) as h:
        boot(h)
        h.key("0")
        sent = ops(h.run(1))[0]
        result(h, sent["request_id"], model.OP_SET, ok=False, error_code="nonzero_exit",
               error="exited with status 1", exit_code=1,
               data={"error": "priority_conflict", "id": "0001", "expected_priority": "p1", "actual_priority": "p2"})
        cmds = h.run(1)
        shown = json.dumps(cmds)
        assert "changed elsewhere" in shown and "Retry p0" in shown
        refresh = ops(cmds)
        assert refresh and refresh[0]["operation_id"] == "stint.list"
        assert not any(o["operation_id"] == "stint.set_priority" for o in refresh)


def test_denied_mutation_keeps_the_board_browsable():
    with AppHarness(str(APP)) as h:
        boot(h)
        h.key("0")
        sent = ops(h.run(1))[0]
        result(h, sent["request_id"], model.OP_SET, ok=False, error_code="denied", error="the approval was denied")
        shown = json.dumps(h.run(1))
        assert "read-only" in shown and "Alpha thing" in shown
        h.key("0")
        assert not any(o["operation_id"] == "stint.set_priority" for o in ops(h.run(1)))
        h.key("l")
        assert ops(h.run(1)), "navigation still reads details"


def test_incompatible_version_blocks_with_install_guidance():
    with AppHarness(str(APP)) as h:
        first = ops(h.run(1))[0]
        result(h, first["request_id"], model.OP_VERSION, data="stint 9.9.9\n")
        shown = json.dumps(h.run(1))
        assert "Incompatible stint version" in shown and "9.9.9" in shown and model.EXPECTED_STINT in shown
        assert not any(o["operation_id"] == "stint.list" for o in ops(h.run(1)))
        h.assert_no_overlap()


def test_search_filters_tasks():
    with AppHarness(str(APP)) as h:
        boot(h)
        h.key("/")
        h.run(1)
        h.text_submit("search", "beta")
        shown = json.dumps(h.run(1))
        assert "Beta thing" in shown and "Alpha thing" not in shown
        assert "Filter" not in shown and "1 of 3 tasks" in shown
        h.key("/")
        h.run(1)
        h.text_submit("search", "gamma")
        h.run(1)
        h.text_submit("search-submit", "gamma")
        kept = json.dumps(h.run(1))
        assert "Filter: gamma" in kept and "Beta thing" not in kept
        h.key("escape")
        assert "Beta thing" in json.dumps(h.run(1))


def test_timer_requests_a_refresh_and_focus_refreshes_immediately():
    with AppHarness(str(APP)) as h:
        boot(h)
        h._send({"type": "timer", "timer_id": str(model.TIMER_REFRESH)})
        sent = ops(h.run(1))
        assert sent and sent[0]["operation_id"] == "stint.list"
