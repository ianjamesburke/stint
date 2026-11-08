"""Pure board model: state in, (state, requests) out. No SDK imports, no I/O.

``main.py`` maps SDK events onto these functions and turns the returned
requests into ``CliOperation`` effects. Every state change that depends on a
stint reply happens here, so the conflict and uncertainty rules are testable
without a host.
"""

import copy
import json
import re

# The Stint CLI release this package is built for. Checked against Cargo.toml.
EXPECTED_STINT = "0.4.1"

PRIORITIES = ["p0", "p1", "p2", "p3", "p4"]
COLUMNS = PRIORITIES + ["unset"]
REFRESH_MS = 5000
TIMER_REFRESH = 1

OP_VERSION = "stint.version"
OP_LIST = "stint.list"
OP_SHOW = "stint.show"
OP_SET = "stint.set_priority"

INSTALL_HELP = "Install the matching CLI and board: see the Stint README, section 'Plexi board'."

# Pre-launch refusals that mean the person said no or the grant is missing.
WRITE_BLOCKING = {"denied", "capability_missing", "disabled"}
# Outcomes where stint may or may not have applied the change.
UNCERTAIN = {"timeout", "cancelled", "output_too_large", "bad_output", "launch_failed"}

_VERSION = re.compile(r"(\d+\.\d+\.\d+)")


def new_state():
    return {
        "phase": "starting",  # starting | ready | incompatible | no_cli | read_denied | broken
        "cli_version": "",
        "message": "",
        "tasks": [],
        "loaded": False,
        "list_error": "",
        "detail": None,
        "detail_error": "",
        "selected": "",
        "query": "",
        "searching": False,
        "write_blocked": False,
        "edit": None,
        "notice": None,
        "seq": 0,
        "queue": [],
        "inflight": None,
    }


def column_of(task):
    return task.get("priority") or "unset"


def visible_tasks(s):
    q = s["query"].strip().lower()
    if not q:
        return list(s["tasks"])
    return [t for t in s["tasks"] if q in t["id"].lower() or q in t["title"].lower()]


def grouped(s):
    cols = {c: [] for c in COLUMNS}
    for t in visible_tasks(s):
        cols[column_of(t)].append(t)
    return cols


def selected_task(s):
    for t in s["tasks"]:
        if t["id"] == s["selected"]:
            return t
    return None


def _notice(s, tone, text):
    s["notice"] = {"tone": tone, "text": text}


def _enqueue(s, op, values, kind):
    for q in s["queue"]:
        if q["op"] == op and q["values"] == values:
            return
    f = s["inflight"]
    if f and f["op"] == op and f["values"] == values and op != OP_SET:
        return
    s["queue"].append({"op": op, "values": values, "kind": kind})


def _pump(s):
    if s["inflight"] is not None or not s["queue"]:
        return []
    nxt = s["queue"].pop(0)
    s["seq"] += 1
    request = {"request_id": f"b-{s['seq']}", "operation_id": nxt["op"], "values": nxt["values"]}
    s["inflight"] = {"request_id": request["request_id"], "op": nxt["op"], "values": nxt["values"], "kind": nxt["kind"]}
    return [request]


def _done(s):
    return s, _pump(s)


def _ready(s):
    return s["phase"] == "ready"


def _enqueue_refresh(s):
    if not _ready(s):
        return
    _enqueue(s, OP_LIST, {}, "list")
    if s["selected"]:
        _enqueue(s, OP_SHOW, {"task": s["selected"]}, "show")


def start(s=None):
    s = new_state() if s is None else copy.deepcopy(s)
    _enqueue(s, OP_VERSION, {}, "version")
    return _done(s)


def refresh(s):
    s = copy.deepcopy(s)
    if s["phase"] in ("read_denied", "broken", "no_cli"):
        s["phase"], s["message"] = "starting", ""
        _enqueue(s, OP_VERSION, {}, "version")
    else:
        _enqueue_refresh(s)
    return _done(s)


def on_tick(s):
    if s["phase"] != "ready":
        return s, []
    return refresh(s)


def on_focus(s):
    return refresh(s)


def _keep_selection_visible(s):
    """After a filter change, never leave the detail on a task the filter hides."""
    visible = grouped(s)
    ids = [t["id"] for c in COLUMNS for t in visible[c]]
    if s["selected"] in ids:
        return s, _pump(s)
    if not ids:
        s["selected"], s["detail"] = "", None
        return s, _pump(s)
    return select(s, ids[0])


def set_query(s, query):
    s = copy.deepcopy(s)
    s["query"] = query
    s["searching"] = True
    return _keep_selection_visible(s)


def end_search(s, clear):
    s = copy.deepcopy(s)
    s["searching"] = False
    if clear:
        s["query"] = ""
    return _keep_selection_visible(s)


def start_search(s):
    s = copy.deepcopy(s)
    s["searching"] = True
    return s, []


def select(s, task_id):
    s = copy.deepcopy(s)
    if s["selected"] != task_id:
        s["selected"] = task_id
        s["detail"], s["detail_error"] = None, ""
        s["edit"] = s["edit"] if s["edit"] and s["edit"]["task"] == task_id else None
        if _ready(s):
            _enqueue(s, OP_SHOW, {"task": task_id}, "show")
    return _done(s)


def move(s, dx, dy):
    """Move the selection: dy within a column, dx between non-empty columns."""
    cols = grouped(s)
    cur = selected_task(s)
    if cur is None:
        first = next((t for c in COLUMNS for t in cols[c]), None)
        return select(s, first["id"]) if first else (s, [])
    col = column_of(cur)
    ci = COLUMNS.index(col)
    row = [t["id"] for t in cols[col]].index(cur["id"]) if cur["id"] in [t["id"] for t in cols[col]] else 0
    if dy:
        items = cols[col]
        row = max(0, min(len(items) - 1, row + dy))
        return select(s, items[row]["id"]) if items else (s, [])
    step = 1 if dx > 0 else -1
    j = ci + step
    while 0 <= j < len(COLUMNS):
        if cols[COLUMNS[j]]:
            items = cols[COLUMNS[j]]
            return select(s, items[min(row, len(items) - 1)]["id"])
        j += step
    return s, []


def request_priority(s, target):
    """Ask stint to move the selected task, guarded by the priority on screen."""
    s = copy.deepcopy(s)
    task = selected_task(s)
    if task is None:
        _notice(s, "warning", "Select a task first.")
        return _done(s)
    if not _ready(s):
        _notice(s, "warning", "The board is not connected to stint yet.")
        return _done(s)
    if s["write_blocked"]:
        _notice(s, "warning", "Read-only: priority changes were not approved. Press e to ask again.")
        return _done(s)
    edit = s["edit"]
    if edit and edit["state"] in ("sending", "reconciling"):
        _notice(s, "warning", "A priority change is still in progress.")
        return _done(s)
    if edit and edit["state"] == "uncertain":
        _notice(s, "warning", "Last change is unconfirmed. Refreshing before any retry.")
        _enqueue_refresh(s)
        return _done(s)
    shown = column_of(task)
    if target == shown:
        _notice(s, "info", f"{task['id']} is already {shown}.")
        return _done(s)
    s["edit"] = {"task": task["id"], "from": shown, "to": target, "state": "sending", "message": ""}
    _notice(s, "info", f"Moving {task['id']} {shown} to {target}...")
    _enqueue(s, OP_SET, {"priority": target, "expected": shown, "task": task["id"]}, "set")
    return _done(s)


def retry(s):
    """Explicit retry of a conflicted or failed change, against the refreshed priority."""
    s2 = copy.deepcopy(s)
    edit = s2["edit"]
    if not edit or edit["state"] not in ("conflict", "failed", "uncertain"):
        _notice(s2, "info", "Nothing to retry.")
        return _done(s2)
    if edit["state"] == "uncertain":
        _enqueue_refresh(s2)
        _notice(s2, "warning", "Verifying the last change before retrying.")
        return _done(s2)
    s2["selected"] = edit["task"]
    task = selected_task(s2)
    if task is None:
        s2["edit"] = None
        _notice(s2, "warning", f"{edit['task']} is no longer on the board.")
        return _done(s2)
    target = edit["to"]
    s2["edit"] = None
    return request_priority(s2, target)


def retry_write(s):
    s = copy.deepcopy(s)
    s["write_blocked"] = False
    _notice(s, "info", "Editing re-enabled. The next change asks for approval again.")
    return _done(s)


def dismiss_notice(s):
    s = copy.deepcopy(s)
    s["notice"] = None
    if s["edit"] and s["edit"]["state"] in ("conflict", "failed"):
        s["edit"] = None
    return s, []


def on_result(s, request_id, ok, data=None, error_code=None, error=None):
    s = copy.deepcopy(s)
    f = s["inflight"]
    if f is None or f["request_id"] != request_id:
        return s, []
    s["inflight"] = None
    kind = f["kind"]
    handler = {"version": _on_version, "list": _on_list, "show": _on_show, "set": _on_set}[kind]
    handler(s, f, ok, data, error_code, error or "")
    return _done(s)


def _read_failed(s, code, error):
    if code == "executable_missing":
        s["phase"], s["message"] = "no_cli", f"stint was not found. {INSTALL_HELP}"
    elif code in ("denied", "capability_missing", "disabled"):
        s["phase"], s["message"] = "read_denied", "Reading tasks was not approved, so the board has nothing to show."
    elif code == "executable_mismatch":
        s["phase"], s["message"] = "incompatible", f"The stint executable changed since it was approved. {INSTALL_HELP}"
    else:
        return False
    return True


def _on_version(s, f, ok, data, code, error):
    if not ok:
        if not _read_failed(s, code, error):
            s["phase"], s["message"] = "broken", f"Could not run stint: {code}: {error}"
        return
    m = _VERSION.search(data if isinstance(data, str) else "")
    found = m.group(1) if m else ""
    s["cli_version"] = found
    if found != EXPECTED_STINT:
        s["phase"] = "incompatible"
        s["message"] = (
            f"This board needs stint {EXPECTED_STINT} but found {found or 'an unreadable version'}. {INSTALL_HELP}"
        )
        return
    s["phase"], s["message"] = "ready", ""
    _enqueue_refresh(s)


def _on_list(s, f, ok, data, code, error):
    edit = s["edit"]
    if not ok:
        if _read_failed(s, code, error):
            return
        s["list_error"] = f"Refresh failed ({code}): {error}"
        if edit and edit["state"] == "reconciling":
            edit["state"] = "uncertain"
        return
    tasks = _parse_tasks(data)
    if tasks is None:
        s["list_error"] = "stint list returned something the board cannot read."
        if edit and edit["state"] == "reconciling":
            edit["state"] = "uncertain"
        return
    s["tasks"], s["loaded"], s["list_error"] = tasks, True, ""
    if s["selected"] and selected_task(s) is None:
        s["selected"], s["detail"] = "", None
    if not s["selected"]:
        first = next((t for c in COLUMNS for t in grouped(s)[c]), None)
        if first:
            s["selected"] = first["id"]
            _enqueue(s, OP_SHOW, {"task": first["id"]}, "show")
    if edit and edit["state"] in ("reconciling", "uncertain"):
        _reconcile(s, edit)


def _reconcile(s, edit):
    task = next((t for t in s["tasks"] if t["id"] == edit["task"]), None)
    now = column_of(task) if task else None
    if now == edit["to"]:
        s["edit"] = None
        _notice(s, "success", f"{edit['task']} is {edit['to']} (confirmed by refresh).")
    elif now == edit["from"]:
        edit["state"] = "failed"
        _notice(s, "warning", f"{edit['task']} is still {edit['from']}: the change was not applied. Press r to retry.")
    else:
        _conflict(s, edit, now)


def _conflict(s, edit, actual):
    edit["state"] = "conflict"
    edit["found"] = actual
    where = actual or "gone"
    _notice(
        s, "warning",
        f"{edit['task']} changed elsewhere: it is {where}, not {edit['from']}. Kept {where}. Press r to retry {edit['to']}.",
    )


def _on_show(s, f, ok, data, code, error):
    if f["values"].get("task") != s["selected"]:
        return
    if ok and isinstance(data, dict):
        s["detail"], s["detail_error"] = data, ""
    elif not _read_failed(s, code, error):
        s["detail"], s["detail_error"] = None, f"Could not load details ({code}): {error}"


def _on_set(s, f, ok, data, code, error):
    edit = s["edit"]
    values = f["values"]
    if edit is None or edit["task"] != values["task"]:
        return
    if ok:
        for t in s["tasks"]:
            if t["id"] == edit["task"]:
                t["priority"] = edit["to"]
        s["edit"] = None
        _notice(s, "success", f"{edit['task']} moved {edit['from']} to {edit['to']}.")
        _enqueue_refresh(s)
        return
    if code == "nonzero_exit" and isinstance(data, dict) and data.get("error") == "priority_conflict":
        _conflict(s, edit, data.get("actual_priority"))
        _enqueue_refresh(s)
        return
    if code in WRITE_BLOCKING:
        s["write_blocked"], s["edit"] = True, None
        _notice(s, "warning", "Read-only: priority changes were not approved. Browsing still works. Press e to ask again.")
        return
    if code in UNCERTAIN:
        edit["state"] = "reconciling"
        edit["message"] = error
        _notice(s, "warning", f"No confirmation for {edit['task']} ({code}). Checking its real priority before any retry.")
        _enqueue_refresh(s)
        return
    edit["state"], edit["message"] = "failed", error
    _notice(s, "error", f"{edit['task']} was not changed ({code}): {error} Press r to retry.")
    _enqueue_refresh(s)


def _parse_tasks(data):
    if not isinstance(data, list):
        return None
    out = []
    for item in data:
        if not isinstance(item, dict) or "id" not in item or "title" not in item:
            return None
        prio = item.get("priority")
        out.append({
            "id": str(item["id"]),
            "title": str(item["title"]),
            "priority": prio if prio in PRIORITIES else None,
            "state": str(item.get("state") or ""),
            "size": item.get("size") or "",
            "blocked": bool(item.get("blocked")),
            "blockers": [str(b) for b in item.get("blockers") or []],
        })
    return out


def dumps(s):
    return json.dumps(s)
