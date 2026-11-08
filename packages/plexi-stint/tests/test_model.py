"""Board model rules: pure state in, (state, requests) out. No host, no stint."""

from app import model

TASKS = [
    {"id": "0001", "title": "Alpha thing", "priority": "p1", "state": "ready", "size": "s", "blocked": False, "blockers": []},
    {"id": "0002", "title": "Beta thing", "priority": "p2", "state": "ready", "size": "m", "blocked": False, "blockers": []},
    {"id": "0003", "title": "Gamma", "priority": None, "state": "proposed", "size": "", "blocked": False, "blockers": []},
]


class Board:
    """Drives the model like the app does and records every request it emits."""

    def __init__(self):
        self.s, self.sent = model.start()

    def apply(self, result):
        self.s, reqs = result
        self.sent += reqs
        return reqs

    def reply(self, ok=True, data=None, code=None, error=None):
        req = self.s["inflight"]
        assert req is not None, "nothing in flight"
        return self.apply(model.on_result(self.s, req["request_id"], ok, data, code, error))

    def drain(self, replies):
        """Answer each in-flight request from {op: reply-kwargs}."""
        while self.s["inflight"]:
            op = self.s["inflight"]["op"]
            self.reply(**replies[op])

    def ready(self, tasks=TASKS):
        self.drain({
            model.OP_VERSION: dict(data=f"stint {model.EXPECTED_STINT}\n"),
            model.OP_LIST: dict(data=[dict(t) for t in tasks]),
            model.OP_SHOW: dict(data={"body": "## Why\nbody", "area": ["cli"]}),
        })
        return self

    def ops(self):
        return [(r["operation_id"], r["values"]) for r in self.sent]


def test_start_checks_version_first_then_lists_and_shows_first_task():
    b = Board()
    assert b.ops() == [(model.OP_VERSION, {})]
    b.ready()
    assert b.ops() == [
        (model.OP_VERSION, {}),
        (model.OP_LIST, {}),
        (model.OP_SHOW, {"task": "0001"}),
    ]
    assert b.s["phase"] == "ready" and b.s["selected"] == "0001"
    assert b.s["detail"]["area"] == ["cli"]


def test_only_one_request_in_flight_and_queue_drains_in_order():
    b = Board()
    assert b.s["inflight"]["op"] == model.OP_VERSION
    assert b.s["queue"] == []
    b.ready()
    assert b.s["inflight"] is None and b.s["queue"] == []


def test_version_mismatch_blocks_everything_with_install_guidance():
    b = Board()
    b.reply(data="stint 0.0.1\n")
    assert b.s["phase"] == "incompatible"
    assert "0.0.1" in b.s["message"] and model.EXPECTED_STINT in b.s["message"]
    assert "README" in b.s["message"]
    assert [o for o, _ in b.ops()] == [model.OP_VERSION]
    assert model.on_tick(b.s)[1] == []


def test_unreadable_version_is_incompatible_not_ready():
    b = Board()
    b.reply(data="garbage")
    assert b.s["phase"] == "incompatible"


def test_missing_cli_and_denied_read_are_distinct_phases():
    b = Board()
    b.reply(ok=False, code="executable_missing", error="not found")
    assert b.s["phase"] == "no_cli"
    b = Board()
    b.reply(ok=False, code="denied", error="no")
    assert b.s["phase"] == "read_denied"
    b.apply(model.refresh(b.s))
    assert b.s["inflight"]["op"] == model.OP_VERSION, "try again restarts at the version check"


def test_grouping_search_and_selection_movement():
    b = Board().ready()
    cols = model.grouped(b.s)
    assert [t["id"] for t in cols["p1"]] == ["0001"]
    assert [t["id"] for t in cols["unset"]] == ["0003"]
    b.apply(model.set_query(b.s, "beta"))
    assert [t["id"] for t in model.visible_tasks(b.s)] == ["0002"]
    assert b.s["selected"] == "0002", "a filter that hides the selection moves it to a visible task"
    b.apply(model.set_query(b.s, "0003"))
    assert [t["id"] for t in model.visible_tasks(b.s)] == ["0003"]
    assert b.s["selected"] == "0003"
    b.apply(model.set_query(b.s, "zzz"))
    assert b.s["selected"] == "" and b.s["detail"] is None
    b.apply(model.end_search(b.s, clear=True))
    assert len(model.visible_tasks(b.s)) == 3
    b.apply(model.select(b.s, "0001"))
    b.apply(model.move(b.s, 1, 0))
    assert b.s["selected"] == "0002", "skips to the next non-empty column"
    b.apply(model.move(b.s, 1, 0))
    assert b.s["selected"] == "0003"
    b.apply(model.move(b.s, 1, 0))
    assert b.s["selected"] == "0003", "edge is a no-op"
    b.apply(model.move(b.s, -1, 0))
    assert b.s["selected"] == "0002"


def test_priority_request_is_guarded_by_the_displayed_priority():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    assert b.ops()[-1] == (model.OP_SET, {"priority": "p0", "expected": "p1", "task": "0001"})
    # UI does not claim success before stint answers
    assert model.selected_task(b.s)["priority"] == "p1"
    assert b.s["edit"]["state"] == "sending"


def test_unset_task_sends_unset_as_the_expected_value():
    b = Board().ready()
    b.apply(model.select(b.s, "0003"))
    b.drain({model.OP_SHOW: dict(data={"body": ""})})
    b.apply(model.request_priority(b.s, "p3"))
    assert b.ops()[-1] == (model.OP_SET, {"priority": "p3", "expected": "unset", "task": "0003"})


def test_success_confirms_only_after_stint_reports_it_then_refreshes():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(data={"updated": "0001", "priority": "p0"})
    assert model.selected_task(b.s)["priority"] == "p0"
    assert b.s["edit"] is None
    assert b.s["notice"]["tone"] == "success"
    assert (model.OP_LIST, {}) in b.ops()[-2:]


def test_same_priority_and_second_change_while_sending_send_nothing():
    b = Board().ready()
    n = len(b.sent)
    b.apply(model.request_priority(b.s, "p1"))
    assert len(b.sent) == n
    b.apply(model.request_priority(b.s, "p0"))
    n = len(b.sent)
    b.apply(model.request_priority(b.s, "p3"))
    assert len(b.sent) == n and b.s["edit"]["to"] == "p0"


CONFLICT = {"error": "priority_conflict", "id": "0001", "expected_priority": "p1", "actual_priority": "p2"}


def test_conflict_keeps_the_newer_value_and_retry_is_explicit():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, data=CONFLICT, code="nonzero_exit", error="exited with status 1")
    assert b.s["edit"]["state"] == "conflict"
    assert b.s["notice"]["text"] == "0001 was changed elsewhere to p2. Your change to p0 wasn't applied. Press R to retry p0."
    sets = [o for o in b.ops() if o[0] == model.OP_SET]
    assert len(sets) == 1, "no automatic retry"
    # the refresh that follows shows the newer value
    other = [dict(t) for t in TASKS]
    other[0]["priority"] = "p2"
    b.drain({model.OP_LIST: dict(data=other), model.OP_SHOW: dict(data={"body": ""})})
    assert model.selected_task(b.s)["priority"] == "p2"
    assert b.s["edit"]["state"] == "conflict"
    b.apply(model.retry(b.s))
    assert b.ops()[-1] == (model.OP_SET, {"priority": "p0", "expected": "p2", "task": "0001"})
    b.reply(data={"updated": "0001", "priority": "p0"})
    assert b.s["edit"] is None


def test_conflict_with_unset_actual_priority_uses_unset_for_retry():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    c = dict(CONFLICT, actual_priority=None)
    b.reply(ok=False, data=c, code="nonzero_exit", error="exit 1")
    other = [dict(t) for t in TASKS]
    other[0]["priority"] = None
    b.drain({model.OP_LIST: dict(data=other), model.OP_SHOW: dict(data={"body": ""})})
    b.apply(model.retry(b.s))
    assert b.ops()[-1][1]["expected"] == "unset"


def test_denied_mutation_goes_read_only_but_browsing_still_works():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, code="denied", error="the approval was denied")
    assert b.s["write_blocked"] and b.s["edit"] is None and b.s["phase"] == "ready"
    n = len(b.sent)
    b.apply(model.request_priority(b.s, "p0"))
    assert len(b.sent) == n, "no request while read-only"
    b.apply(model.move(b.s, 1, 0))
    b.apply(model.refresh(b.s))
    assert [o for o, _ in b.ops()[n:]][-1] in (model.OP_LIST, model.OP_SHOW)
    b.apply(model.retry_write(b.s))
    b.apply(model.request_priority(b.s, "p0"))
    b.drain({model.OP_LIST: dict(data=[dict(t) for t in TASKS]), model.OP_SHOW: dict(data={"body": ""}),
             model.OP_SET: dict(data={"updated": "0002", "priority": "p0"})})
    assert (model.OP_SET, {"priority": "p0", "expected": "p2", "task": "0002"}) in b.ops()


def test_timeout_is_uncertain_and_reconciled_before_any_retry_applied_case():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, code="timeout", error="exceeded 10000 ms")
    assert b.s["edit"]["state"] == "reconciling"
    assert "Checking its real priority" in b.s["notice"]["text"]
    n = len(b.sent)
    b.apply(model.request_priority(b.s, "p3"))
    assert [o for o in b.ops()[n:] if o[0] == model.OP_SET] == [], "no retry while unconfirmed"
    landed = [dict(t) for t in TASKS]
    landed[0]["priority"] = "p0"
    b.drain({model.OP_LIST: dict(data=landed), model.OP_SHOW: dict(data={"body": ""})})
    assert b.s["edit"] is None
    assert "confirmed by refresh" in b.s["notice"]["text"]


def test_timeout_reconciles_to_not_applied_and_allows_explicit_retry():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, code="timeout", error="exceeded")
    b.drain({model.OP_LIST: dict(data=[dict(t) for t in TASKS]), model.OP_SHOW: dict(data={"body": ""})})
    assert b.s["edit"]["state"] == "failed"
    assert "not applied" in b.s["notice"]["text"]
    b.apply(model.retry(b.s))
    assert b.ops()[-1] == (model.OP_SET, {"priority": "p0", "expected": "p1", "task": "0001"})


def test_timeout_then_unverifiable_refresh_stays_uncertain():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, code="timeout", error="exceeded")
    b.reply(ok=False, code="timeout", error="exceeded")
    assert b.s["edit"]["state"] == "uncertain"
    assert b.s["list_error"]
    n = len(b.sent)
    b.apply(model.retry(b.s))
    assert [o for o in b.ops()[n:] if o[0] == model.OP_SET] == []
    assert b.ops()[-1][0] in (model.OP_LIST, model.OP_SHOW)


def test_pre_launch_failure_says_nothing_changed_and_allows_retry():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, code="approval_timeout", error="no decision")
    assert b.s["edit"]["state"] == "failed"
    assert "was not changed" in b.s["notice"]["text"]
    assert not b.s["write_blocked"]


def test_tick_and_focus_refresh_but_do_not_pile_up():
    b = Board().ready()
    b.apply(model.on_focus(b.s))
    b.apply(model.on_tick(b.s))
    b.apply(model.on_tick(b.s))
    lists = [o for o in b.ops() if o[0] == model.OP_LIST]
    assert len(lists) == 2, "one startup list plus one in-flight refresh; duplicates are collapsed"
    b.drain({model.OP_LIST: dict(data=[dict(t) for t in TASKS]), model.OP_SHOW: dict(data={"body": ""})})
    b.apply(model.on_focus(b.s))
    assert b.s["inflight"]["op"] == model.OP_LIST


def test_agent_change_appears_after_refresh_and_stale_results_are_ignored():
    b = Board().ready()
    b.apply(model.refresh(b.s))
    moved = [dict(t) for t in TASKS]
    moved[1]["priority"] = "p0"
    b.reply(data=moved)
    assert [t["id"] for t in model.grouped(b.s)["p0"]] == ["0002"]
    assert model.on_result(b.s, "b-999", True, [], None, None)[1] == []


def test_selected_task_disappearing_reselects():
    b = Board().ready()
    b.apply(model.refresh(b.s))
    b.reply(data=[dict(TASKS[1])])
    assert b.s["selected"] == "0002"


def test_unreadable_list_output_is_reported_not_swallowed():
    b = Board().ready()
    b.apply(model.refresh(b.s))
    b.reply(data={"not": "a list"})
    assert "cannot read" in b.s["list_error"]


def test_periodic_refresh_is_slow_and_only_while_focused():
    assert model.REFRESH_MS >= 30000
    b = Board().ready()
    n = len(b.sent)
    b.apply(model.on_tick(b.s))
    assert len(b.sent) == n, "focus is unknown until a FocusGained event: no periodic refresh"
    b.apply(model.on_focus(b.s))
    b.drain({model.OP_LIST: dict(data=[dict(t) for t in TASKS]), model.OP_SHOW: dict(data={"body": ""})})
    b.apply(model.on_blur(b.s))
    n = len(b.sent)
    b.apply(model.on_tick(b.s))
    assert len(b.sent) == n, "never refresh while unfocused"
    b.apply(model.on_focus(b.s))
    assert b.ops()[-1][0] == model.OP_LIST, "refocus refreshes immediately"
    b.drain({model.OP_LIST: dict(data=[dict(t) for t in TASKS]), model.OP_SHOW: dict(data={"body": ""})})
    b.apply(model.on_tick(b.s))
    assert b.s["inflight"]["op"] == model.OP_LIST


def test_every_mutation_outcome_triggers_a_refresh():
    for reply in (dict(data={"updated": "0001", "priority": "p0"}),
                  dict(ok=False, data=CONFLICT, code="nonzero_exit", error="x"),
                  dict(ok=False, code="approval_timeout", error="x")):
        b = Board().ready()
        b.apply(model.request_priority(b.s, "p0"))
        b.reply(**reply)
        assert b.s["inflight"] and b.s["inflight"]["op"] == model.OP_LIST


def test_task_removed_elsewhere_gets_its_own_wording():
    b = Board().ready()
    b.apply(model.request_priority(b.s, "p0"))
    b.reply(ok=False, code="timeout", error="x")
    b.drain({model.OP_LIST: dict(data=[dict(t) for t in TASKS[1:]]), model.OP_SHOW: dict(data={"body": ""})})
    assert "was removed elsewhere" in b.s["notice"]["text"]
