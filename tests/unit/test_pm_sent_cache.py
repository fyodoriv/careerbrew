"""Sent-day control locks do not change a cached view's business inputs."""

from careerbrew import server, store
from careerbrew.day import pm_learn


def test_empty_sent_lock_keeps_business_version(home):
    before = server.files_version()
    with pm_learn._sent_lock("2026-10-19"):
        pass
    assert server.files_version() == before


def test_sent_lock_exclusion_keeps_real_inputs_visible(home):
    store.write_text(home / "day" / "seed.md", "synthetic seed")
    changes = [
        ("day/pm-sent-2026-10-19.md", "synthetic sent text"),
        ("day/pm-sent-receipt-2026-10-19.json", '{"id":"synthetic receipt"}'),
        ("day/.pm-sent-2026-10-19.lock.extra", None),
        ("work/.pm-sent-2026-10-19.lock", None),
        ("day/.pm-sent-2026-10-19.lock", "synthetic unexpected content"),
    ]
    for relative, content in changes:
        before = server.files_version()
        path = home / relative
        if content is None:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        else:
            store.write_text(path, content)
        assert server.files_version() != before, relative


def test_empty_pm_history_lock_keeps_business_version(home):
    lock = home / "day" / ".pm-edits.jsonl.lock"
    lock.parent.mkdir(parents=True)
    before = server.files_version()
    lock.touch()
    assert server.files_version() == before

    for relative, body in (
        ("day/pm-edits.jsonl", "synthetic history"),
        ("day/.pm-edits.jsonl.lock.extra", ""),
        ("work/.pm-edits.jsonl.lock", ""),
        ("day/.pm-edits.jsonl.lock", "synthetic unexpected content"),
    ):
        before = server.files_version()
        store.write_text(home / relative, body)
        assert server.files_version() != before, relative
