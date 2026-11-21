"""Registered373: two backend primaries, six guards and two AFTER-only contracts.

Each subhistory owns its synthetic data, clocks and retained evidence. This
module changes no application function. Registration57e37ab0 precedes it.
"""

from __future__ import annotations

import hashlib
import json
import socket
import stat
import subprocess
import uuid
import webbrowser
from pathlib import Path

import pytest

DAY = "2026-11-02"
RAW = "billing: next: reconcile amber invoices.\nOther: reviewed 4 PRs."
CUT = RAW.splitlines()[0]
ROOT = Path(__file__).resolve().parents[2]
UNSUPPORTED = (
    "[Other](https://example.com): reviewed 4 PRs.",
    "# Other: reviewed 4 PRs.",
    "> Other: reviewed 4 PRs.",
    "```text\nOther: reviewed 4 PRs.\n```",
    "1. Other: reviewed 4 PRs.",
    "an unlabelled nonblank paragraph",
)


def snapshot(root: Path) -> dict[str, object]:
    return {
        str(p.relative_to(root)): {
            "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
            "mtime_ns": p.stat().st_mtime_ns,
            "mode": stat.S_IMODE(p.stat().st_mode),
        }
        for p in sorted(root.rglob("*"))
        if p.is_file()
    }


def sample(*, only_other: bool = False, label: str = "billing") -> dict:
    planned = [] if only_other else [{"title": "reconcile amber invoices", "project": label}]
    return {
        "day": DAY,
        "tomorrow": "2026-11-03",
        "done": {"Shipped": ["Reviewed 4 pull requests"]},
        "planned": [],
        "blockers": [],
        "details": {"done": {"Shipped": [{"kind": "github", "repo": "", "project": ""}]}, "planned": planned},
    }


class CutWorld:
    """One subhistory owns an encrypted data root and its evidence record."""

    def __init__(self, folder: Path, monkeypatch, evidence: Path):
        from careerbrew import paths, store

        self.root, self.home, self.evidence = folder, folder / "data", evidence
        self.store = store
        folder.mkdir(mode=0o700, parents=True)
        (folder / ".owned-373-synthetic").write_text("registered373 synthetic subhistory\n")
        for name in ("client", "data", "keys", "tmp", "state", "cache", "config", "xdg-data", "runtime", "http", "commands", "backups", "public-cards"):
            (folder / name).mkdir(parents=True)
        (folder / "git-global").write_text("")
        self.environment = {
            "HOME": folder / "client",
            "CAREERBREW_HOME": self.home,
            "CAREERBREW_KEY_FILE": folder / "keys/backup.key",
            "CAREERBREW_PAIR_KEY_FILE": folder / "keys/pair.key",
            "CAREERBREW_CLAUDE_CONFIG": folder / "client/claude.json",
            "CAREERBREW_CODEX_CONFIG": folder / "client/codex.toml",
            "CAREERBREW_COMMANDS_DIR": folder / "commands",
            "CAREERBREW_BACKUP_DIRS": folder / "backups",
            "CAREERBREW_PUBLIC_CARDS": folder / "public-cards",
            "TMPDIR": folder / "tmp",
            "XDG_STATE_HOME": folder / "state",
            "XDG_CONFIG_HOME": folder / "config",
            "XDG_CACHE_HOME": folder / "cache",
            "XDG_DATA_HOME": folder / "xdg-data",
            "XDG_RUNTIME_DIR": folder / "runtime",
            "OP_CONFIG_DIR": folder / "client/op",
            "GIT_CONFIG_GLOBAL": folder / "git-global",
            "CAREERBREW_HTTP_FIXTURES": folder / "http",
        }
        for key, value in self.environment.items():
            monkeypatch.setenv(key, str(value))
        for key, value in {
            "CAREERBREW_EVENTS": "off",
            "CAREERBREW_LLM": "off",
            "CAREERBREW_NOTIFY": "off",
            "CAREERBREW_POSTHOG_KEY": "",
            "CAREERBREW_TELEMETRY_URL": "http://127.0.0.1:9",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_COUNT": "0",
            "UV_OFFLINE": "1",
            "UV_NO_SYNC": "1",
            "UV_PYTHON_DOWNLOADS": "never",
        }.items():
            monkeypatch.setenv(key, value)
        monkeypatch.delenv("CAREERBREW_TEST_OWNER", raising=False)
        self.clock = lambda value: monkeypatch.setenv("CAREERBREW_NOW", value)
        self.clock("2026-11-02T15:00:00-05:00")
        store.write_yaml(self.home / "settings.yaml", {"timezone": "America/New_York"})
        store.write_yaml(self.home / "profile.yaml", {"current_level": 3})
        assert paths.data_root().resolve() == self.home.resolve()
        assert Path.home().resolve().is_relative_to(folder.resolve())

    def activate(self, monkeypatch) -> None:
        for key, value in self.environment.items():
            monkeypatch.setenv(key, str(value))

    def edit(self, day: str, when: str, *, raw: str = RAW, shown: str | None = None, body: str = CUT) -> None:
        from careerbrew.day import pm_learn

        self.clock(when)
        pm_learn.remember(day, raw, raw if shown is None else shown)
        pm_learn.record(day, body)

    def cuts(self, *, raw: str = RAW, body: str = CUT) -> None:
        from careerbrew.day import pm_learn

        for day in ("2026-10-26", "2026-10-27", "2026-10-28"):
            self.edit(day, day + "T15:00:00-04:00", raw=raw, body=body)
        self.clock("2026-11-02T15:00:00-05:00")
        assert pm_learn.rules()["cut"], "BLOCKED: real cut lesson is not active"

    def logical(self) -> dict:
        from careerbrew.day import log, pm_learn

        return {
            "edits": self.store.read_jsonl(pm_learn.path()),
            "resets": self.store.read_json(pm_learn.resets_path(), {}),
            "draft": self.store.read_text(log.pm_draft_path(DAY)),
            "sent": self.store.read_text(pm_learn.sent_path(DAY)),
            "receipt": self.store.read_json(pm_learn.receipt_path(DAY), {}),
            "tasks": self.store.read_yaml(self.home / "work/tasks.yaml", {}),
            "notes": self.store.read_text(self.home / "work/owner-notes.md"),
        }

    def prove(self, name: str, value: dict) -> None:
        self.evidence.mkdir(parents=True, exist_ok=True)
        path = self.evidence / f"{name}.json"
        assert not path.exists()
        path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")


@pytest.fixture
def cut_world(home, tmp_path, monkeypatch, request):
    from careerbrew import actions, connectors, secrets
    from tests.safety.issue373_metadata import source_hashes

    denied = []
    source_before = source_hashes()

    def deny(*_args, **_kwargs):
        denied.append("forbidden host operation")
        raise AssertionError("BLOCKED: transport, secret, browser or child process attempted")

    # Same boundary as the existing synthetic data-show fixture; no file-read hooks.
    for target, name in (
        (subprocess, "run"),
        (subprocess, "Popen"),
        (socket, "create_connection"),
        (socket.socket, "connect"),
        (socket.socket, "connect_ex"),
        (socket.socket, "sendto"),
        (secrets, "resolve"),
        (connectors, "http_json"),
        (connectors, "http_get"),
        (webbrowser, "open"),
    ):
        monkeypatch.setattr(target, name, deny)
    retained = ROOT / ".metrics/issue373/unit-retained"
    assert not any(p.is_symlink() for p in (ROOT / ".metrics", retained.parent, retained)), "BLOCKED: proof parent is a symlink"
    evidence = retained / (request.node.name + "-" + uuid.uuid4().hex)
    token = actions.OWNER_UI.set(False)
    worlds = []

    def make(name: str = "main") -> CutWorld:
        world = CutWorld(tmp_path / name, monkeypatch, evidence / name)
        worlds.append(world)
        return world

    try:
        yield make
    finally:
        try:
            for world in worlds:
                world.activate(monkeypatch)
                world.prove("terminal-fixture", {"forbidden_attempts": denied, "snapshot": snapshot(world.root), "logical": world.logical(), "owned_child_spawns": 0, "source_sha256": source_before})
        finally:
            actions.OWNER_UI.reset(token)
        assert source_hashes() == source_before, "BLOCKED: public source changed during case"
        assert denied == []


@pytest.mark.parametrize("case", ["P01", "P02"])
def test_pm_cut_primary(cut_world, case):
    from careerbrew.day import log, pm_learn

    world = cut_world()
    world.cuts()
    d = sample(only_other=case == "P02")
    raw = "\n".join(log.pm_lines(d, learn=False))
    assert raw == ("Other: reviewed 4 PRs." if case == "P02" else RAW), "BLOCKED: actual builder input differs"
    initial = log.pm_report(d)
    if case == "P01":
        assert "Other:" not in initial["body"]
        log.save_pm_draft(DAY, raw)
        assert "Other" in pm_learn.rules()["cut"], "BLOCKED: restoration erased the lesson precondition"
    else:
        assert initial["body"] == raw and pm_learn.apply(raw.splitlines(), set()) == raw.splitlines()
    expected = world.logical()
    before = snapshot(world.root)
    actual = log.pm_report(d)
    assert actual["body"] == raw and "Other:" in actual["body"]
    assert world.logical() == expected and snapshot(world.root) == before, "BLOCKED: settled projection changed encrypted state"
    world.prove("before-final-oracle", {"case": case, "raw": raw, "pm": actual, "cut_still_active": "Other" in pm_learn.rules()["cut"], "preserved": True})
    assert actual["left_out"] == [], {"case": case, "visible_label_flagged_absent": actual["left_out"]}


@pytest.mark.parametrize("case", ["C01", "C02", "C03", "C04", "C05", "C06"])
def test_pm_cut_guard(cut_world, capsys, case):
    from careerbrew import actions, cli, store
    from careerbrew.day import log, pm_learn

    if case == "C01":
        for name, restored in (("absent", None), ("same-sentence-other-label", "billing: reviewed 4 PRs.")):
            w = cut_world(name)
            w.cuts()
            log.pm_report(sample())
            if restored is not None:
                log.save_pm_draft(DAY, restored)
            assert "Other" in pm_learn.rules()["cut"]
            pm = log.pm_report(sample())
            w.prove("guard", {"pm": pm})
            assert "Other:" not in pm["body"] and pm["left_out"] == ["Other"]
    elif case == "C02":
        for n, label in enumerate(("Roundup", "Équipe")):
            w = cut_world(f"rename-{n}")
            body = RAW.replace("Other:", label + ":")
            for day in ("2026-10-26", "2026-10-27"):
                w.edit(day, day + "T15:00:00-04:00", body=body)
            assert pm_learn.rules()["rename"] == {"Other": label}
            w.clock("2026-11-02T15:00:00-05:00")
            for text in (body, body.replace(label + ":", f"- **{label}**:")):
                log.pm_report(sample())
                log.save_pm_draft(DAY, text)
                pm = log.pm_report(sample())
                assert pm["body"] == text and pm["left_out"] == []
            w.prove("guard", {"pm": pm, "rename": label})
    elif case == "C03":
        w = cut_world()
        w.cuts()
        assert cli.main(["signals", "reset", "cut", "Other"]) == 0
        pm = log.pm_report(sample())
        assert not pm_learn.rules()["cut"] and "Other:" in pm["body"] and pm["left_out"] == []
        assert store.read_json(pm_learn.resets_path())["other"]
        w.prove("guard", {"pm": pm, "history": w.logical()})
    elif case == "C04":
        w = cut_world()
        pm = log.pm_report(sample())
        assert pm["body"] == pm["draft"] == RAW and pm["left_out"] == [] and store.read_jsonl(pm_learn.path()) == []
        assert not (pm.get("cut_context") or {}).get("candidates")
        w.prove("guard", {"pm": pm})
    elif case == "C05":
        w = cut_world()
        w.cuts()
        assert not actions.OWNER_UI.get() and not cli.sys.stdout.isatty() and not cli.may_show_business()
        capsys.readouterr()
        assert cli.main(["signals", "--json"]) == 0
        output = capsys.readouterr().out
        assert "Other" not in output and "billing" not in output and "recorded_at" not in output and "report_day" not in output
        assert json.loads(output)
        w.prove("guard", {"restricted_output": output, "business_gate": False})
    else:
        w = cut_world()
        w.cuts()
        store.write_text(w.home / "work/owner-notes.md", "synthetic note remains separate")
        store.write_yaml(w.home / "work/tasks.yaml", {"tasks": []})
        store.write_text(pm_learn.sent_path(DAY), "synthetic existing sent update\n")
        store.write_json(pm_learn.receipt_path(DAY), {"id": "slack:C373:373.000001"})
        settled = log.pm_report(sample())
        before, logical = snapshot(w.root), w.logical()
        for _ in range(3):
            assert log.pm_report(sample()) == settled
        assert snapshot(w.root) == before and w.logical() == logical
        w.prove("guard", {"pm": settled, "snapshot_preserved": True, "logical": logical})


@pytest.mark.parametrize("case", ["D01", "D02"])
def test_pm_cut_dates(cut_world, case):
    """AFTER-only; these two contracts are never selected for the old baseline."""
    from careerbrew import store
    from careerbrew.day import log, pm_learn

    if case == "D01":
        w = cut_world()
        for day in ("2026-10-18", "2026-10-19"):
            w.edit(day, "2026-10-29T15:00:00-04:00")
        w.clock("2026-11-01T09:00:00-05:00")
        pm_learn.reset("Other")
        w.edit("2026-10-20", "2026-11-01T15:00:00-05:00")
        w.edit("2026-10-21", "2026-11-02T15:00:00-05:00")
        w.edit("2026-10-31", "2026-11-03T15:00:00-05:00", shown=CUT, body=CUT + " Additional unrelated detail.")
        w.clock("2026-11-04T15:00:00-05:00")
        assert "Other" in pm_learn.rules()["cut"]
        log.pm_report(sample())
        before = snapshot(w.root)
        pm = log.pm_report(sample())
        assert snapshot(w.root) == before
        candidate = next(c for c in pm["cut_context"]["candidates"] if c["label"] == "Other")
        from datetime import datetime

        assert datetime.fromisoformat(candidate["recorded_at"]).isoformat() == "2026-11-02T20:00:00+00:00"
        assert candidate["report_day"] == "2026-10-21" and candidate["date_basis"] == "saved"
        w.prove("dates", {"pm": pm, "history": w.logical()})
    else:
        for n, saved in enumerate((None, "not-a-time", "2026-10-27T15:00:00")):
            w = cut_world(f"legacy-{n}")
            w.cuts()
            rows = store.read_jsonl(pm_learn.path())[:2]
            for row in rows:
                row.pop("saved", None)
                if saved is not None:
                    row["saved"] = saved
            store.write_jsonl(pm_learn.path(), rows)
            assert "Other" in pm_learn.rules()["cut"]
            log.pm_report(sample())
            before = snapshot(w.root)
            pm = log.pm_report(sample())
            assert snapshot(w.root) == before
            candidate = next(c for c in pm["cut_context"]["candidates"] if c["label"] == "Other")
            assert candidate["recorded_at"] is None and candidate["date_basis"] == "report-day"
            assert candidate["report_day"] in {"2026-10-26", "2026-10-27"}
            w.prove("legacy", {"pm": pm})
        w = cut_world("legacy-no-dates")
        w.cuts()
        rows = store.read_jsonl(pm_learn.path())[:2]
        for n, row in enumerate(rows):
            row.pop("saved", None)
            if n == 0:
                row["day"] = "invalid-day"
            else:
                row.pop("day", None)
        store.write_jsonl(pm_learn.path(), rows)
        assert "Other" in pm_learn.rules()["cut"]
        log.pm_report(sample())
        before = snapshot(w.root)
        pm = log.pm_report(sample())
        assert snapshot(w.root) == before
        candidate = next(c for c in pm["cut_context"]["candidates"] if c["label"] == "Other")
        assert candidate["recorded_at"] is None and candidate["report_day"] is None and candidate["date_basis"] == "unavailable"
        w.prove("legacy", {"pm": pm})
        for n, text in enumerate(("Other: reviewed 4 PRs.", "- **Other**: reviewed 4 PRs.", "• _Other_: reviewed 4 PRs.", "oThEr: reviewed 4 PRs.", "Different: reviewed 4 PRs.", *UNSUPPORTED)):
            w = cut_world(f"ascii-comparison-{n}")
            w.cuts()
            log.pm_report(sample())
            log.save_pm_draft(DAY, CUT + "\n" + text)
            assert "Other" in pm_learn.rules()["cut"]
            pm = log.pm_report(sample())
            candidates = pm["cut_context"]["candidates"]
            assert any(c["label"] == "Other" for c in candidates)
            expected = ["Other"] if text.startswith("Different:") else []
            assert pm["left_out"] == expected
            w.prove("comparison", {"pm": pm, "input": text, "expected_absent": expected})
        unicode_d = sample()
        unicode_d["details"]["done"]["Shipped"][0]["project"] = "Équipe"
        unicode_raw = RAW.replace("Other:", "Équipe:")
        for n, label in enumerate(("Équipe", "ÉQUIPE", "E\u0301quipe")):
            w = cut_world(f"unicode-comparison-{n}")
            w.cuts(raw=unicode_raw)
            assert "\n".join(log.pm_lines(unicode_d, learn=False)) == unicode_raw
            log.pm_report(unicode_d)
            log.save_pm_draft(DAY, unicode_raw.replace("Équipe:", label + ":"))
            assert "Équipe" in pm_learn.rules()["cut"]
            pm = log.pm_report(unicode_d)
            assert any(c["label"] == "Équipe" for c in pm["cut_context"]["candidates"])
            assert pm["left_out"] == []
            w.prove("comparison", {"pm": pm, "input_label": label, "known_exact": label == "Équipe"})
