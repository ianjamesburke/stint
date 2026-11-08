"""Stint Board: priority board over approved typed stint CLI operations.

All task data and every change goes through the four declared operations in
manifest.toml. State transitions live in app/model.py; this file only maps SDK
events to the model and model requests to effects.
"""

from plexi_sdk import log, state
from plexi_sdk.effects import CliOperation, SetState, SetTimer, SetTitle
from plexi_sdk.events import (
    CliOperationResult,
    FocusGained,
    KeyEvent,
    TimerFired,
    UiAction,
    UiValueChange,
)

from app import model
from app import ui as app_ui

KEY_PRIORITY = {"0": "p0", "1": "p1", "2": "p2", "3": "p3", "4": "p4"}


def _board():
    return state.get("board") or model.new_state()


def _effects(new_state, requests):
    out = [SetState({"board": new_state})]
    for r in requests:
        log.info(f"Stint Board: request {r['request_id']} {r['operation_id']}")
        out.append(CliOperation(request_id=r["request_id"], operation_id=r["operation_id"], values=r["values"]))
    return out


def init(size, args):
    log.info("Stint Board: init")
    s, requests = model.start()
    return [SetTitle("Stint Board"), SetTimer(id=model.TIMER_REFRESH, delay_ms=model.REFRESH_MS, repeat=True), *_effects(s, requests)]


def _key(s, event):
    if s["searching"]:
        if event.key == "escape":
            return model.end_search(s, clear=True)
        if event.key == "enter":
            return model.end_search(s, clear=False)
        return None
    key = event.key
    if key in KEY_PRIORITY:
        return model.request_priority(s, KEY_PRIORITY[key])
    if key in ("j", "down"):
        return model.move(s, 0, 1)
    if key in ("k", "up"):
        return model.move(s, 0, -1)
    if key in ("h", "left"):
        return model.move(s, -1, 0)
    if key in ("l", "right"):
        return model.move(s, 1, 0)
    if key == "/":
        return model.start_search(s)
    if key == "escape":
        return model.dismiss_notice(model.end_search(s, clear=True)[0])
    if key == "r":
        edit = s["edit"]
        if edit and edit["state"] in ("conflict", "failed", "uncertain"):
            return model.retry(s)
        return model.refresh(s)
    if key == "e":
        return model.retry_write(s)
    return None


def _action(s, handler_id):
    if handler_id.startswith("prio:"):
        return model.request_priority(s, handler_id.split(":", 1)[1])
    if handler_id == "search-done":
        return model.end_search(s, clear=False)
    if handler_id == "search-clear":
        return model.end_search(s, clear=True)
    if handler_id == "retry":
        return model.retry(s)
    if handler_id == "refresh":
        return model.refresh(s)
    if handler_id.startswith("pick-"):
        column, _, idx = handler_id[len("pick-"):].partition(":")
        tasks = model.grouped(s).get(column, [])
        if idx.isdigit() and int(idx) < len(tasks):
            return model.select(s, tasks[int(idx)]["id"])
    return None


def update(event):
    s = _board()
    result = None
    if isinstance(event, KeyEvent) and event.pressed:
        result = _key(s, event)
    elif isinstance(event, UiAction):
        result = _action(s, event.handler_id)
    elif isinstance(event, UiValueChange) and event.handler_id == "search":
        result = model.set_query(s, event.value)
    elif isinstance(event, UiValueChange) and event.handler_id == "search-submit":
        result = model.end_search(model.set_query(s, event.value)[0], clear=False)
    elif isinstance(event, TimerFired) and event.id == model.TIMER_REFRESH:
        result = model.on_tick(s)
    elif isinstance(event, FocusGained):
        result = model.on_focus(s)
    elif isinstance(event, CliOperationResult):
        result = model.on_result(s, event.request_id, event.ok, event.data, event.error_code, event.error)
    if result is None:
        return []
    return _effects(*result)


def view():
    return app_ui.build_view(_board())
