"""`careerbrew data show <file>`: read one data file without changing anything (issue #103).

Full text prints redacted; business files (work/, day/, ladders/, out/promo-*) print only where
business content may (a terminal, the web app, or the plugin policy). `--keys` prints structure
only: key names and value types, never a value, so agents can check a file's shape.
"""

import json

import pytest

from careerbrew import actions, plugins, store, vault
from careerbrew.cli import main

from .conftest import CANARIES


def _snapshot(home):
    """Bytes and mtime of every file under the data root, except the content-free event log."""
    return {p.relative_to(home): (p.read_bytes(), p.stat().st_mtime_ns) for p in sorted(home.rglob("*")) if p.is_file() and p.relative_to(home).parts[0] != "logs"}


def _connectors(home):
    path = home / "work" / "connectors.yaml"
    store.write_yaml(
        path,
        {
            "connectors": [
                {"kind": "jira", "token": f"keychain:{CANARIES[2].lower()}.jira", "base_url": f"https://{CANARIES[2].lower()}.example", "port": 443},
                {"kind": "github", "token": "keychain:careerbrew.github", "user": CANARIES[0], "verify": True, "note": None},
            ]
        },
    )
    return path


def test_show_prints_a_decrypted_file_and_changes_nothing(home, capsys):
    store.write_yaml(home / "settings.yaml", {"ladder": "generic-swe", "work_end": "18:00"})
    assert vault.is_sealed((home / "settings.yaml").read_bytes()), "the fixture is encrypted on disk"
    before = _snapshot(home)
    assert main(["data", "show", "settings.yaml"]) == 0
    out = capsys.readouterr().out
    assert "ladder: generic-swe" in out and "work_end: '18:00'" in out
    assert _snapshot(home) == before, "show writes nothing: same files, same bytes, same mtimes"
    assert vault.is_sealed((home / "settings.yaml").read_bytes()), "the file stays encrypted"


def test_show_redacts_sensitive_terms(home, capsys):
    store.write_yaml(home / "profile.yaml", {"name": CANARIES[0], "employer": CANARIES[1], "current_level": 3})
    assert main(["data", "show", "profile.yaml"]) == 0
    out = capsys.readouterr().out
    assert "current_level: 3" in out and "[REDACTED]" in out
    assert CANARIES[0] not in out and CANARIES[1] not in out


def test_show_refuses_business_text_to_a_pipe_and_points_to_keys(home, capsys):
    _connectors(home)
    before = _snapshot(home)
    assert main(["data", "show", "work/connectors.yaml"]) == 3
    out = capsys.readouterr().out
    assert "--keys" in out and "agent_may_read_business" in out
    for secret in (CANARIES[2].lower(), "keychain", "jira", "443"):
        assert secret not in out
    assert _snapshot(home) == before


@pytest.mark.parametrize("rel", ["day/log/2026-10-20.md", "ladders/acme.yaml", "out/promo-acme-L5.md"])
def test_every_business_folder_is_gated(home, capsys, rel):
    store.write_text(home / rel, f"title: {CANARIES[2]}\n")  # out/ stays plaintext by design; the rest is sealed
    assert main(["data", "show", rel]) == 3
    assert CANARIES[2] not in capsys.readouterr().out


@pytest.mark.parametrize(("rel", "real"), [("WORK/connectors.yaml", "work"), ("Work/connectors.yaml", "work"), ("OUT/promo-acme-L5.md", "out"), ("out/PROMO-acme-L5.md", "out")])
def test_a_business_path_in_another_case_is_still_gated(home, capsys, rel, real):
    """macOS opens WORK/ as work/: the gate must not fail open on the caller's case (review of #103)."""
    _connectors(home)
    store.write_text(home / "out" / "promo-acme-L5.md", f"# Promo\n{CANARIES[2]}\n")
    case_insensitive = (home / real.upper()).exists()
    rc = main(["data", "show", rel])
    out = capsys.readouterr().out
    assert rc == (3 if case_insensitive else 1), out
    assert CANARIES[2] not in out and CANARIES[2].lower() not in out


def test_only_personal_paths_print_to_a_pipe(home, capsys):
    """An allowlist, not a denylist: cache/, plugins/, out/, and any new folder are business until listed (review of #103)."""
    items, _hit = store.cached(["git-evidence", "/repos/x", "abc123", []], lambda: [{"title": f"ship {CANARIES[2]} billing"}])
    assert items and (home / "cache").is_dir()
    cache_file = next((home / "cache").glob("*.json"))
    assert vault.is_sealed(cache_file.read_bytes()), "the cache is encrypted on disk"
    gated = [
        f"cache/{cache_file.name}",
        "plugins/acme/ladders/acme.yaml",
        "out/dashboard.html",
        "research/pages/abc.json",
        "newfolder/notes.yaml",
    ]
    for rel in gated[1:]:
        store.write_text(home / rel, f"title: {CANARIES[2]}\n")
    for rel in gated:
        assert main(["data", "show", rel]) == 3, rel
        assert CANARIES[2] not in capsys.readouterr().out, rel
    assert main(["data", "show", f"cache/{cache_file.name}", "--keys"]) == 0
    out = capsys.readouterr().out
    assert "value[].title: str" in out and CANARIES[2] not in out
    store.write_yaml(home / "jobs" / "criteria.yaml", {"min_salary": 150000})
    assert main(["data", "show", "jobs/criteria.yaml"]) == 0, "a personal folder still prints"
    assert "min_salary: 150000" in capsys.readouterr().out


def test_keys_never_prints_names_that_key_a_business_map(home, capsys):
    """work/projects.yaml is keyed by project name; a people map by username. Short, identifier-shaped
    keys pass the free-text check, so in a business file a map under a named key prints as `*` (review of #103)."""
    zebra = CANARIES[2].lower()
    store.write_yaml(
        home / "work" / "projects.yaml",
        {
            "projects": {
                zebra: {"created_at": "2026-10-01T09:00:00Z", "stale_after_days": 7, "sources": [{"kind": "doc", "ref": "r1"}]},
                f"{zebra}-two": {"created_at": "2026-10-02T09:00:00Z", "owner": "me"},
            },
            "people": {CANARIES[0]: 3},
        },
    )
    assert main(["data", "show", "work/projects.yaml", "--keys"]) == 0
    out = capsys.readouterr().out
    assert out.splitlines()[1:] == [
        "projects: map[2]",
        "projects.*: map",
        "projects.*.created_at: str",
        "projects.*.stale_after_days: int",
        "projects.*.sources: list",
        "projects.*.sources[]: map",
        "projects.*.sources[].kind: str",
        "projects.*.sources[].ref: str",
        "projects.*.owner: str",
        "people: map[1]",
        "people.*: int",
    ], out
    assert zebra not in out.lower() and CANARIES[0] not in out
    r = actions.run("data.show", {"file": "work/projects.yaml", "keys": True})
    assert r["exit"] == 0 and f"projects.{zebra}: map" in r["output"], "the owner, who may read the text, gets the names"


def test_business_text_prints_when_the_plugin_policy_allows_it(home, capsys, monkeypatch):
    _connectors(home)
    store.write_yaml(home / "profile.yaml", {"name": CANARIES[0]})  # the owner's name is a sensitive term
    monkeypatch.setattr(plugins, "effective_policy", lambda: {"agent_may_read_business": True})
    assert main(["data", "show", "work/connectors.yaml"]) == 0
    out = capsys.readouterr().out
    assert "kind: jira" in out and "base_url:" in out
    assert CANARIES[0] not in out, "still redacted"


def test_the_owner_sees_business_text_in_the_web_app(home):
    _connectors(home)
    r = actions.run("data.show", {"file": "work/connectors.yaml"})
    assert r["exit"] == 0 and "kind: jira" in r["output"]
    assert actions.run("data.show", {"file": "work/connectors.yaml", "keys": True})["exit"] == 0


def test_keys_prints_structure_and_never_a_value(home, capsys):
    _connectors(home)
    before = _snapshot(home)
    assert main(["data", "show", "work/connectors.yaml", "--keys"]) == 0, "structure is not business content: it prints to a pipe"
    out = capsys.readouterr().out
    lines = out.splitlines()
    assert lines[0] == "work/connectors.yaml: yaml, encrypted on disk, 10 line(s)"
    assert lines[1:] == [
        "connectors: list[2]",
        "connectors[]: map",
        "connectors[].kind: str",
        "connectors[].token: str",
        "connectors[].base_url: str",
        "connectors[].port: int",
        "connectors[].user: str",
        "connectors[].verify: bool",
        "connectors[].note: null",
    ]
    for value in (CANARIES[0], CANARIES[2].lower(), "jira", "github", "keychain", "443", "True"):
        assert value not in out
    assert _snapshot(home) == before


def test_keys_collapses_free_text_keys_and_long_maps(home, capsys):
    data = {
        "cards": {"What is a heap? (explain it to a junior)": {"due": "2026-10-23", "reps": 2}, CANARIES[2]: {"due": "2026-10-24", "reps": 1, "lapses": 0}},
        "by_id": {f"t{i:03d}": i for i in range(40)},
        "rows": [[1, "a"], [2, None]],
    }
    store.write_text(home / "tutor" / "state.json", json.dumps(data))
    assert main(["data", "show", "tutor/state.json", "--keys"]) == 0
    out = capsys.readouterr().out
    assert out.splitlines()[1:] == [
        "cards: map[2]",
        "cards.*: map",
        "cards.*.due: str",
        "cards.*.reps: int",
        "cards.*.lapses: int",
        "by_id: map[40]",
        "by_id.*: int",
        "rows: list[2]",
        "rows[]: list",
        "rows[][]: int|str|null",
    ], out
    assert "What is a heap" not in out and CANARIES[2] not in out and "t000" not in out


def test_keys_on_jsonl_and_text_files(home, capsys):
    store.write_text(home / "work" / "evidence.jsonl", "".join(json.dumps(r) + "\n" for r in ({"title": CANARIES[2], "tags": ["delivery"]}, {"title": "second", "at": 3})))
    assert main(["data", "show", "work/evidence.jsonl", "--keys"]) == 0
    out = capsys.readouterr().out
    assert out.splitlines() == [
        "work/evidence.jsonl: jsonl, encrypted on disk, 2 record(s)",
        "[]: map",
        "[].title: str",
        "[].tags: list",
        "[].tags[]: str",
        "[].at: int",
    ]
    store.write_text(home / "day" / "log" / "2026-10-20.md", f"# Day\n\nShipped {CANARIES[2]}\n")
    assert main(["data", "show", "day/log/2026-10-20.md", "--keys"]) == 0
    out = capsys.readouterr().out
    assert out.splitlines() == ["day/log/2026-10-20.md: text, encrypted on disk, 3 line(s) (no keys: not YAML, JSON, or JSONL)"]


def test_keys_on_a_broken_file_names_the_line_not_the_content(home, capsys):
    store.write_text(home / "work" / "feed-rules.yaml", f"rules:\n  - match: {CANARIES[2]}\n    bad: [unclosed\n")
    assert main(["data", "show", "work/feed-rules.yaml", "--keys"]) == 1
    out = capsys.readouterr().out
    assert "cannot parse as yaml" in out and "line" in out and CANARIES[2] not in out


@pytest.mark.parametrize("rel", ["../outside.yaml", "missing.yaml", "work"])
def test_show_refuses_paths_that_are_not_data_files(home, capsys, tmp_path, rel):
    (home / "work").mkdir(parents=True, exist_ok=True)
    (tmp_path / "outside.yaml").write_text("a: 1\n")
    assert main(["data", "show", rel]) == 1
    assert "no such data file" in capsys.readouterr().out


def test_agents_are_told_to_use_keys():
    a = next(x for x in actions.catalog() if x["name"] == "data.show")
    assert a["ui"] and not a["confirm"] and "--keys" in (a["agent_note"] or "")
