"""Count sent PM history coverage without exporting or writing source data."""

import json
import stat
from datetime import date, timedelta
from pathlib import Path
from runpy import run_path

import pytest

from careerbrew import backup, plugins, store
from careerbrew.day import pm_learn

SCRIPT = Path(__file__).resolve().parents[2] / "scripts/pm-sent-coverage.py"
FIRST = date(2026, 11, 1)
DRAFT = "Synthetic original draft"
EDIT = "Synthetic distinct final body"
RECEIPT = {"channel": "C0PRIVATE", "ts": "1793539800.000001"}


def day(index):
    return (FIRST + timedelta(days=index)).isoformat()


def snapshot(home):
    entries = {"data": home, "key-parent": backup.key_path().parent, "key": backup.key_path()}
    entries.update({f"data/{p.relative_to(home)}": p for p in home.rglob("*")})
    result = {}
    for name, path in entries.items():
        if not path.exists():
            result[name] = None
            continue
        info = path.stat()
        result[name] = (stat.S_IFMT(info.st_mode), stat.S_IMODE(info.st_mode), info.st_mtime_ns, path.read_bytes() if path.is_file() else None)
    return result


def defects(missing=0, stale=0, elsewhere=0, duplicates=0):
    return {"missing_history": missing, "stale_history": stale, "wrong_day_history": elsewhere, "duplicate_days": duplicates}


@pytest.fixture
def audit(monkeypatch):
    monkeypatch.setattr(plugins, "effective_policy", lambda **_: {"agent_may_read_business": True})
    return run_path(str(SCRIPT))["main"]


def test_sent_coverage_counts_defects_and_exclusions(audit, home, capsys):
    for index in (0, 1, 2, 3, 4, 6):
        pm_learn.remember(day(index), DRAFT, DRAFT)
    for index in (0, 1, 2, 3, 4, 5):
        pm_learn.save_sent(day(index), DRAFT if index == 4 else EDIT, receipt=RECEIPT)
    pm_learn.save_sent(day(6), EDIT)
    store.write_jsonl(
        pm_learn.path(),
        [
            {"day": day(0), "from": "sent", "body": EDIT},
            {"day": day(2), "from": "sent", "body": "Synthetic stale text"},
            {"day": day(3), "from": "web", "body": EDIT},
            {"day": day(7), "from": "sent", "body": EDIT},
        ],
    )
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(6), "--json"]) == 0
    out = capsys.readouterr()
    assert json.loads(out.out) == {
        "status": "ok",
        "covered": 1,
        "eligible": 4,
        "rate": 0.25,
        "excluded": {"no_shown_draft": 1, "unidentified_source": 1, "unchanged": 1},
        "defects": defects(stale=1, elsewhere=2),
    }
    assert not out.err
    for value in (DRAFT, EDIT, RECEIPT["channel"], RECEIPT["ts"], day(0)):
        assert value not in out.out
    assert snapshot(home) == before


def test_zero_denominator_is_insufficient(audit, home, capsys):
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 0
    result = json.loads(capsys.readouterr().out)
    assert (result["status"], result["eligible"], result["rate"]) == ("insufficient_data", 0, None)
    assert result["defects"] == defects()
    assert snapshot(home) == before


def test_denied_audit_opens_no_business_record(audit, home, monkeypatch, capsys):
    pm_learn.remember(day(0), DRAFT, DRAFT)
    pm_learn.save_sent(day(0), EDIT, receipt=RECEIPT)
    before = snapshot(home)
    monkeypatch.setattr(plugins, "effective_policy", lambda **_: {"agent_may_read_business": False})

    def unexpected(*args, **kwargs):
        pytest.fail("audit read a business record without permission")

    monkeypatch.setattr(store, "read_text", unexpected)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 2
    assert "requires" in capsys.readouterr().err
    assert snapshot(home) == before


def test_real_policy_denies_without_creating_a_data_root(home, capsys):
    audit = run_path(str(SCRIPT))["main"]
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 2
    assert "requires" in capsys.readouterr().err
    assert not home.exists()
    assert snapshot(home) == before


def test_busy_sent_lock_returns_incomplete_without_writes(audit, home, capsys):
    import fcntl

    pm_learn.remember(day(0), DRAFT, DRAFT)
    pm_learn.save_sent(day(0), EDIT, receipt=RECEIPT)
    lock = home / "day" / f".pm-sent-{day(0)}.lock"
    before = snapshot(home)
    with lock.open("rb") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        assert audit(["--from", day(0), "--to", day(0), "--json"]) == 1
    assert json.loads(capsys.readouterr().out) == {"status": "incomplete"}
    assert snapshot(home) == before


def test_missing_key_stays_missing(audit, home, capsys):
    from careerbrew import backup

    pm_learn.remember(day(0), DRAFT, DRAFT)
    key = backup.key_path()
    key.unlink()
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 1
    assert json.loads(capsys.readouterr().out) == {"status": "incomplete"}
    assert not key.exists()
    assert snapshot(home) == before


def test_ambiguous_retained_slack_source_is_excluded(audit, home, capsys):
    from careerbrew import config
    from careerbrew.work import feed

    pm_learn.remember(day(0), DRAFT, DRAFT)
    config.set_value("pm_slack", "#synthetic-destination")
    ids = ["slack:C0PRIVATE:1793539800.000001", "slack:C0PRIVATE:1793539800.000002"]
    store.write_jsonl(feed.events_path(), [{"id": ident, "source": "slack", "actor": "me", "channel": "synthetic-destination", "at": f"{day(0)}T16:00:00Z"} for ident in ids])
    store.write_jsonl(feed.originals_path(), [{"id": ident, "text": EDIT} for ident in ids])
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 0
    result = json.loads(capsys.readouterr().out)
    assert result["excluded"]["unidentified_source"] == 1
    assert result["eligible"] == 0
    assert snapshot(home) == before


def test_real_business_policy_authorizes_without_changing_files(home, capsys):
    audit = run_path(str(SCRIPT))["main"]
    store.write_json(home / "plugins.lock.json", {"plugins": {"fixture": {}}})
    store.write_yaml(home / "plugins" / "fixture" / plugins.MANIFEST, {"policy": {"agent_may_read_business": True}})
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["status"] == "insufficient_data"
    assert snapshot(home) == before


def test_audit_discards_counts_when_an_input_changes(audit, home, capsys, monkeypatch):
    pm_learn.remember(day(0), DRAFT, DRAFT)
    pm_learn.save_sent(day(0), EDIT, receipt=RECEIPT)
    read = store.read_json
    changed = False

    def concurrent_write(path, *args, **kwargs):
        nonlocal changed
        result = read(path, *args, **kwargs)
        if path == pm_learn.shown_path(day(0)) and not changed:
            changed = True
            pm_learn.remember(day(0), DRAFT, "Synthetic replacement by another writer")
        return result

    monkeypatch.setattr(store, "read_json", concurrent_write)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 1
    assert changed
    assert json.loads(capsys.readouterr().out) == {"status": "incomplete"}


def test_malformed_settings_do_not_leak_the_source_line(audit, home, capsys):
    store.write_text(home / "settings.yaml", "private-destination-canary: [")
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 1
    out = capsys.readouterr()
    assert json.loads(out.out) == {"status": "incomplete"}
    assert out.err == ""
    assert snapshot(home) == before


def retained_positive(audit, home, capsys, *, direct):
    from careerbrew import config
    from careerbrew.work import feed

    body = DRAFT + " synthetic-project-canary retained edit"
    target = "synthetic-recipient" if direct else "#synthetic-channel"
    pm_learn.remember(day(0), DRAFT, DRAFT)
    config.set_value("pm_slack", target)
    identity = "slack:C0PRIVATE:1793539800.000003"
    store.write_jsonl(feed.events_path(), [{"id": identity, "source": "slack", "actor": "me", "channel": "synthetic-channel", "at": f"{day(0)}T16:00:00Z"}])
    store.write_jsonl(feed.originals_path(), [{"id": identity, "text": body, "original": {"channel": {"is_im": True, "is_mpim": False, "name": target}}}])
    store.write_jsonl(pm_learn.path(), [{"day": day(0), "from": "sent", "body": body}])
    assert not pm_learn.sent_path(day(0)).exists()
    assert not pm_learn.receipt_path(day(0)).exists()
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 0
    out = capsys.readouterr()
    assert json.loads(out.out) == {"status": "ok", "covered": 1, "eligible": 1, "rate": 1.0, "excluded": {"no_shown_draft": 0, "unidentified_source": 0, "unchanged": 0}, "defects": defects()}
    assert not out.err
    for canary in (DRAFT, body, target, identity, "synthetic-project-canary"):
        assert canary not in out.out
    assert snapshot(home) == before


def test_unique_retained_changed_channel_is_covered(audit, home, capsys):
    retained_positive(audit, home, capsys, direct=False)


def test_unique_retained_changed_dm_is_covered(audit, home, capsys):
    retained_positive(audit, home, capsys, direct=True)


def test_duplicate_and_distinct_missing_history_subcases(audit, home, tmp_path, monkeypatch, capsys):
    for label in ("duplicate", "missing", "elsewhere"):
        with monkeypatch.context() as context:
            root = tmp_path / label / "data"
            context.setenv("CAREERBREW_HOME", str(root))
            context.setenv("CAREERBREW_KEY_FILE", str(tmp_path / label / "key" / "backup.key"))
            body = EDIT + " " + label
            pm_learn.remember(day(0), DRAFT, DRAFT)
            pm_learn.save_sent(day(0), body, receipt=RECEIPT)
            rows = {
                "duplicate": [{"day": day(0), "from": "sent", "body": "Earlier distinct body"}, {"day": day(0), "from": "sent", "body": body}],
                "missing": [{"day": day(7), "from": "sent", "body": "Unrelated distinct body"}],
                "elsewhere": [{"day": day(7), "from": "sent", "body": body}],
            }[label]
            store.write_jsonl(pm_learn.path(), rows)
            before = snapshot(root)
            assert audit(["--from", day(0), "--to", day(0), "--json"]) == 0
            out = capsys.readouterr()
            result = json.loads(out.out)
            assert (result["eligible"], result["covered"]) == (1, int(label == "duplicate"))
            assert result["defects"] == {"duplicate": defects(duplicates=1), "missing": defects(missing=1), "elsewhere": defects(elsewhere=1)}[label]
            assert not out.err
            assert body not in out.out
            assert snapshot(root) == before


def test_latest_stale_duplicate_is_not_covered(audit, home, capsys):
    pm_learn.remember(day(0), DRAFT, DRAFT)
    pm_learn.save_sent(day(0), EDIT, receipt=RECEIPT)
    store.write_jsonl(pm_learn.path(), [{"day": day(0), "from": "sent", "body": EDIT}, {"day": day(0), "from": "sent", "body": "Later stale distinct body"}])
    before = snapshot(home)
    assert audit(["--from", day(0), "--to", day(0), "--json"]) == 0
    out = capsys.readouterr()
    result = json.loads(out.out)
    assert (result["eligible"], result["covered"]) == (1, 0)
    assert result["defects"] == defects(stale=1, duplicates=1)
    assert EDIT not in out.out
    assert not out.err
    assert snapshot(home) == before
