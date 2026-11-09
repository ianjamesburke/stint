"""Release pins: board version, Stint CLI version and Plexi floor stay in step."""

import tomllib
from pathlib import Path

from app import model

APP_DIR = Path(__file__).resolve().parent.parent
REPO = APP_DIR.parent.parent
MANIFEST = tomllib.loads((APP_DIR / "manifest.toml").read_text())


def test_board_version_matches_the_stint_cli_version():
    cargo = tomllib.loads((REPO / "Cargo.toml").read_text())
    assert MANIFEST["app"]["version"] == cargo["package"]["version"] == model.EXPECTED_STINT


def test_minimum_plexi_version_is_declared():
    assert MANIFEST["requires"]["plexi_min"] == "0.4.4", "first Plexi with 1175 and 1180"


def test_read_and_mutation_authority_are_separate_operations():
    ops = {op["id"]: op["authority"]["mode"] for op in MANIFEST["cli_operation"]}
    assert ops == {
        "stint.version": "read",
        "stint.list": "read",
        "stint.show": "read",
        "stint.set_priority": "mutate",
    }


def test_every_operation_runs_stint_and_never_a_shell():
    for op in MANIFEST["cli_operation"]:
        assert op["executable"] == "stint"
        assert "-c" not in op["argv"]


def test_free_text_slots_sit_after_the_end_of_options_delimiter():
    for op in MANIFEST["cli_operation"]:
        argv = op["argv"]
        if "{task}" in argv:
            assert argv.index("--") < argv.index("{task}")


def test_board_declares_no_file_or_network_authority_beyond_the_workspace():
    for op in MANIFEST["cli_operation"]:
        auth = op["authority"]
        assert auth.get("net_hosts", []) == []
        assert set(auth.get("fs_read", [])) <= {"workspace"}
        assert set(auth.get("fs_write", [])) <= {"workspace"}
        assert "fs_write" not in auth or auth["mode"] == "mutate"


def test_app_source_never_touches_stint_files_or_a_shell():
    for path in [APP_DIR / "main.py", *(APP_DIR / "app").glob("*.py")]:
        text = path.read_text()
        for banned in ("subprocess", "os.system", ".stint", "open(", "FileRead", "FileWrite"):
            assert banned not in text, f"{path.name} uses {banned}"
