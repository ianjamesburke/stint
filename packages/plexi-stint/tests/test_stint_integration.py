"""The board's declared operations against a real stint binary and a temp repo.

``HostSim`` stands in for the Plexi host only in building argv from the
manifest template and mapping the process result onto the documented
``CliOperationResult`` shape. Everything stint does is the real binary.
"""

import json
import os
import re
import shutil
import subprocess
import tomllib
from pathlib import Path

import pytest

from app import model

APP_DIR = Path(__file__).resolve().parent.parent
REPO = APP_DIR.parent.parent
MANIFEST = tomllib.loads((APP_DIR / "manifest.toml").read_text())
OPS = {op["id"]: op for op in MANIFEST["cli_operation"]}


def _stint_bin():
    override = os.environ.get("STINT_BIN")
    if override:
        return override
    built = REPO / "target" / "debug" / "stint"
    if built.exists():
        return str(built)
    found = shutil.which("stint")
    if found:
        return found
    pytest.skip("no stint binary: run `cargo build` in the stint repo or set STINT_BIN")


STINT = _stint_bin()
_SLOT = re.compile(r"^(--[\w-]+=)?\{(\w+)\}$")


def render_argv(op, values):
    out = []
    for element in op["argv"]:
        m = _SLOT.match(element)
        if m:
            out.append((m.group(1) or "") + str(values[m.group(2)]))
        else:
            out.append(element)
    return out


class HostSim:
    def __init__(self, cwd):
        self.cwd = cwd
        self.calls = []

    def run(self, request):
        op = OPS[request["operation_id"]]
        assert set(request["values"]) == set(op.get("slots", {})), "values must be exactly the declared slots"
        for name, slot in op.get("slots", {}).items():
            if slot["type"] == "enum":
                assert request["values"][name] in slot["values"]
        argv = render_argv(op, request["values"])
        self.calls.append(argv)
        proc = subprocess.run([STINT, *argv], cwd=self.cwd, capture_output=True, text=True, timeout=30)
        fmt = op["output"]["format"]
        out = proc.stdout
        assert len(out.encode()) <= op["output"]["max_bytes"], "operation output bound too small"

        def parse(text):
            if fmt == "json":
                try:
                    return json.loads(text)
                except ValueError:
                    return None
            return text

        if proc.returncode == 0:
            return dict(ok=True, data=parse(out))
        return dict(ok=False, data=parse(out), code="nonzero_exit", error=f"exited with status {proc.returncode}: {proc.stderr[:512]}")


class Rig:
    def __init__(self, repo):
        self.sim = HostSim(repo)
        self.s, reqs = model.start()
        self.pending = list(reqs)
        self.repo = repo

    def settle(self):
        while self.s["inflight"]:
            req = self.s["inflight"]
            res = self.sim.run({"operation_id": req["op"], "values": req["values"]})
            self.s, more = model.on_result(self.s, req["request_id"], res["ok"], res.get("data"), res.get("code"), res.get("error"))
        return self

    def do(self, fn, *args):
        self.s, _ = fn(self.s, *args)
        return self.settle()


def cli(repo, *args):
    return subprocess.run([STINT, *args], cwd=repo, capture_output=True, text=True, check=True).stdout


@pytest.fixture
def repo(tmp_path):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    cli(tmp_path, "init")
    cli(tmp_path, "add", "First", "--priority", "p1", "--size", "s", "--no-edit")
    cli(tmp_path, "add", "Second", "--priority", "p2", "--size", "m", "--no-edit")
    cli(tmp_path, "add", "Third unprioritised", "--no-edit")
    return tmp_path


def priority_of(repo, task_id):
    return json.loads(cli(repo, "show", "--json", "--", task_id))["priority"]


def test_installed_stint_matches_the_version_this_board_pins():
    out = subprocess.run([STINT, "--version"], capture_output=True, text=True, check=True).stdout
    assert model.EXPECTED_STINT in out, f"{STINT} reports {out!r}; board pins {model.EXPECTED_STINT}"


def test_startup_lists_real_tasks_and_loads_detail(repo):
    r = Rig(repo).settle()
    assert r.s["phase"] == "ready"
    by_id = {t["id"]: t["priority"] for t in r.s["tasks"]}
    assert by_id == {"0001": "p1", "0002": "p2", "0003": None}
    assert r.s["detail"]["id"] == "0001" and "body" in r.s["detail"]


def test_board_change_is_visible_to_the_agent_cli(repo):
    r = Rig(repo).settle()
    r.do(model.request_priority, "p0")
    assert r.s["edit"] is None and r.s["notice"]["tone"] == "success"
    assert priority_of(repo, "0001") == "p0"


def test_unset_task_can_be_prioritised_with_the_unset_guard(repo):
    r = Rig(repo).settle()
    r.do(model.select, "0003")
    r.do(model.request_priority, "p4")
    assert priority_of(repo, "0003") == "p4"
    assert any("--expect-priority=unset" in call for call in r.sim.calls)


def test_cli_change_appears_after_refresh(repo):
    r = Rig(repo).settle()
    cli(repo, "set", "0002", "--priority", "p0")
    r.do(model.refresh)
    assert {t["id"]: t["priority"] for t in r.s["tasks"]}["0002"] == "p0"


def test_conflict_preserves_the_newer_value_then_explicit_retry_applies(repo):
    r = Rig(repo).settle()
    cli(repo, "set", "0001", "--priority", "p3")  # an agent edits while the board still shows p1
    assert model.selected_task(r.s)["priority"] == "p1"
    r.do(model.request_priority, "p0")
    assert priority_of(repo, "0001") == "p3", "stale write must not clobber the newer value"
    assert r.s["edit"]["state"] == "conflict"
    assert model.selected_task(r.s)["priority"] == "p3", "board converges on the newer value"
    assert "changed elsewhere to p3" in r.s["notice"]["text"] and "wasn't applied" in r.s["notice"]["text"]
    r.do(model.retry)
    assert priority_of(repo, "0001") == "p0"
    assert r.s["edit"] is None


def test_conflict_when_task_was_unset_in_the_meantime(repo):
    r = Rig(repo).settle()
    cli(repo, "set", "0001", "--priority", "")
    r.do(model.request_priority, "p0")
    assert priority_of(repo, "0001") is None
    assert r.s["edit"]["state"] == "conflict"
    r.do(model.retry)
    assert priority_of(repo, "0001") == "p0"


def test_task_id_that_looks_like_a_flag_is_never_an_option(repo):
    op = OPS["stint.show"]
    argv = render_argv(op, {"task": "--help"})
    assert argv[-2:] == ["--", "--help"]
    proc = subprocess.run([STINT, *argv], cwd=repo, capture_output=True, text=True)
    assert proc.returncode != 0 and "Usage" not in proc.stdout
