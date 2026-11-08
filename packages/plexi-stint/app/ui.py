"""View builders: pure functions from the board model to a component tree."""

from plexi_sdk.ui import (
    SPACE_MD,
    SPACE_SM,
    SPACE_XS,
    Actions,
    AppBar,
    Banner,
    Button,
    Card,
    Column,
    EmptyState,
    FooterKeys,
    Heading,
    HStack,
    KeyValue,
    Scrollable,
    SelectList,
    Text,
    TextInput,
)

from app import model

TONES = {"info": "accent", "success": "success", "warning": "warning", "error": "danger"}
STATE_TONE = {"conflict": "warning", "failed": "danger", "uncertain": "warning"}


def _subtitle(s):
    if s["phase"] != "ready":
        return ""
    n = len(s["tasks"])
    shown = len(model.visible_tasks(s))
    count = f"{shown} of {n} tasks" if shown != n else f"{n} tasks"
    return f"stint {s['cli_version']} | {count}"


def _banner(s):
    out = []
    if s["notice"]:
        out.append(Banner(s["notice"]["text"], tone=TONES.get(s["notice"]["tone"], "neutral")))
    if s["list_error"]:
        out.append(Banner(s["list_error"], tone="warning"))
    if s["write_blocked"]:
        out.append(Banner("Read-only: priority changes were not approved.", tone="neutral", title="read-only"))
    return out


def _short(title, limit=20):
    return title if len(title) <= limit else title[: limit - 1] + "\u2026"


def _column(s, name, tasks):
    label = "unset" if name == "unset" else name.upper()
    rows = [
        Button(
            f"{t['id']} {_short(t['title'])}",
            on_click=f"pick:{t['id']}",
            style="primary" if t["id"] == s["selected"] else "ghost",
        )
        for t in tasks
    ] or [Text("none", tone="hint")]
    return Column([Heading(f"{label} ({len(tasks)})", truncate=True), *rows], gap=SPACE_XS)


def _detail(s):
    task = model.selected_task(s)
    if task is None:
        return Text("Select a task with j/k/h/l or click one.", tone="hint")
    shown = model.column_of(task)
    busy = bool(s["edit"] and s["edit"]["state"] in ("sending", "reconciling"))
    locked = s["write_blocked"] or busy or s["phase"] != "ready"
    buttons = [
        Button(p, on_click=f"prio:{p}", style="primary" if p == shown else "secondary", disabled=locked)
        for p in model.PRIORITIES
    ]
    edit = s["edit"]
    if edit and edit["state"] in ("conflict", "failed", "uncertain"):
        buttons.append(Button(f"Retry {edit['to']}", on_click="retry", style="primary"))
    rows = [("id", task["id"]), ("status", task["state"]), ("priority", shown)]
    if task["size"]:
        rows.append(("size", task["size"]))
    if task["blockers"]:
        rows.append(("blocked by", ", ".join(task["blockers"])))
    detail = s["detail"]
    body = []
    if s["detail_error"]:
        body.append(Text(s["detail_error"], color="warning"))
    elif detail is None:
        body.append(Text("Loading details...", tone="hint"))
    else:
        rows.extend((k, ", ".join(detail[k])) for k in ("area",) if detail.get(k))
        body.append(Text(str(detail.get("body") or "").strip() or "(no body)", monospace=True))
    return Column(
        [Heading(task["title"], truncate=True), KeyValue(rows), Actions(buttons), Scrollable(Column(body, gap=SPACE_SM))],
        gap=SPACE_SM,
    )


def _board(s):
    cols = model.grouped(s)
    if s["loaded"] and not s["tasks"]:
        return EmptyState("No open tasks", "stint list returned no active tasks in this workspace.")
    if not s["loaded"]:
        return Text("Loading tasks...", tone="hint")
    return HStack([_column(s, c, cols[c]) for c in model.COLUMNS], gap=SPACE_SM, align="start")


def _search(s):
    if s["searching"]:
        return HStack([
            Column([TextInput("search", value=s["query"], on_change="search", on_submit="search-submit",
                              placeholder="Search id or title (Enter to keep)", autofocus=True)], grow=True),
            Actions([Button("Done", on_click="search-done", style="primary"), Button("Clear", on_click="search-clear")]),
        ], gap=SPACE_SM)
    if s["query"]:
        return Text(f"Filter: {s['query']}  (/ to edit, Esc to clear)", tone="hint")
    return Text("/ to search", tone="hint")


FOOTER = [
    ("hjkl", "move"),
    ("0-4", "set priority"),
    ("r", "retry/refresh"),
    ("/", "search"),
    ("e", "re-enable edits"),
]


def build_view(s):
    blocked = s["phase"] in ("incompatible", "no_cli", "read_denied", "broken")
    if blocked:
        title = {
            "incompatible": "Incompatible stint version",
            "no_cli": "stint not found",
            "read_denied": "Reading tasks was not approved",
            "broken": "Cannot reach stint",
        }[s["phase"]]
        main = [EmptyState(title, s["message"]), Actions([Button("Try again", on_click="refresh", style="primary")])]
    elif s["phase"] == "starting":
        main = [Text("Connecting to stint...", tone="hint")]
    else:
        main = [_search(s), _board(s), _detail(s)]
    return Column(
        [AppBar("Stint Board", _subtitle(s)), *_banner(s), *main, FooterKeys(FOOTER)],
        grow=True,
        padding=SPACE_MD,
        gap=SPACE_SM,
    )
