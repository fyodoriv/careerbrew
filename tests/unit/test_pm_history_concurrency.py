"""Force shared PM history writers to overlap after reading their snapshots."""

from queue import SimpleQueue
from threading import Event, Thread, current_thread

import pytest

from careerbrew import store, vault
from careerbrew.day import log, pm_learn

FIRST = "2026-11-02"
SECOND = "2026-11-03"
DRAFT = "Today\n• Synthetic draft"
SENT = "Today\n• Synthetic sent edit"
OTHER = "Today\n• Synthetic second edit"


@pytest.mark.parametrize("second_source", ["sent", "web"])
def test_pm_history_retains_both_concurrent_writers(second_source, monkeypatch):
    for day in (FIRST, SECOND):
        pm_learn.remember(day, DRAFT, DRAFT)
    pm_learn.save_sent(FIRST, SENT)
    if second_source == "sent":
        pm_learn.save_sent(SECOND, OTHER)

    first_at_write = Event()
    second_started = Event()
    second_wrote = Event()
    release = Event()
    errors = SimpleQueue()
    write = store.write_jsonl
    history = pm_learn.path()

    def ordered_write(path, rows):
        name = current_thread().name
        if path == history and name == "first-writer":
            first_at_write.set()
            if not release.wait(12):
                raise TimeoutError("first writer was not released")
        write(path, rows)
        if path == history and name == "second-writer":
            second_wrote.set()

    def first():
        assert pm_learn.learn_sent(FIRST)

    def second():
        second_started.set()
        if second_source == "sent":
            assert pm_learn.learn_sent(SECOND)
        else:
            log.save_pm_draft(SECOND, OTHER)

    def capture(fn):
        try:
            fn()
        except BaseException as error:
            errors.put(error)

    monkeypatch.setattr(store, "write_jsonl", ordered_write)
    a = Thread(target=capture, args=(first,), name="first-writer", daemon=True)
    b = Thread(target=capture, args=(second,), name="second-writer", daemon=True)
    a.start()
    try:
        assert first_at_write.wait(8), "first writer did not read the history"
        b.start()
        assert second_started.wait(8), "second writer did not enter"
        overlapped = second_wrote.wait(2)
    finally:
        release.set()
        a.join(8)
        if b.ident is not None:
            b.join(8)
    assert not a.is_alive() and not b.is_alive(), "writers did not terminate"
    if not errors.empty():
        raise errors.get()

    assert vault.is_sealed(history.read_bytes())
    rows = store.read_jsonl(history)
    assert [(row["day"], row["from"], row["body"]) for row in rows] == [
        (FIRST, "sent", SENT),
        (SECOND, second_source, OTHER),
    ], f"second wrote before first release: {overlapped}"


def test_pm_history_web_save_preserves_sent_authority_and_mtime():
    pm_learn.remember(FIRST, DRAFT, DRAFT)
    pm_learn.save_sent(FIRST, SENT)
    assert pm_learn.learn_sent(FIRST)
    history = pm_learn.path()
    before = history.read_bytes(), history.stat().st_mtime_ns
    log.save_pm_draft(FIRST, OTHER)
    assert not pm_learn.learn_sent(FIRST)
    assert (history.read_bytes(), history.stat().st_mtime_ns) == before
    assert store.read_jsonl(history)[0]["body"] == SENT
