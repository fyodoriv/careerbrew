"""Reply drafts fit the thread's newest state and are saved safely (issue #269, US-98)."""

from __future__ import annotations

import json
import threading
import time

import pytest

from careerbrew import cli, events, store
from careerbrew.work import drafts

from .conftest import CANARIES


def _thread(*texts: str, mine_last: bool = False) -> list[dict]:
    rows = [{"ts": f"1700000{n:03d}.000100", "text": t, "mine": False} for n, t in enumerate(texts)]
    if mine_last:
        rows[-1]["mine"] = True
    return drafts.messages({"messages": rows})


# Content rules


def test_a_thread_whose_newest_message_says_a_deploy_started_yields_an_ack_not_a_change_request():
    thread = _thread("Here is the PR for the new flag.", "Deploy started, will update when done.")
    r = drafts.fit(thread, draft="Could you rename the flag before we ship?")
    assert r["kind"] == "ack"
    assert r["deploy_started"] is True
    assert r["text"] == drafts.DEFAULT_ACK
    assert not drafts._CHANGE.search(r["text"])
    assert "asked for changes" in r["reason"]


def test_an_approving_draft_is_kept_after_a_deploy_started():
    r = drafts.fit(_thread("Rolling out to production now."), draft="LGTM, thanks!")
    assert (r["kind"], r["keep_draft"], r["text"]) == ("ack", True, None)


def test_a_major_issue_stops_the_release_in_one_line_and_a_long_reason_is_refused():
    r = drafts.fit(_thread("Release kicked off."), draft="nit: rename it", major="the migration drops the users table")
    assert r["kind"] == "stop"
    assert r["text"] == "Please hold the release: the migration drops the users table"
    with pytest.raises(drafts.DraftError, match="one line"):
        drafts.fit(_thread("Release kicked off."), major="x" * 201)


@pytest.mark.parametrize(
    "text",
    ["We will deploy tomorrow.", "Can we release this?", "The release will start after lunch.", "We are about to start the deploy.", "Did the deploy start?", "The fix is not deployed yet.", "Hasn't been released."],
)
def test_a_planned_or_asked_about_deploy_is_not_a_started_one(text):
    r = drafts.fit(_thread(text), draft="Could you add a test first?")
    assert r["deploy_started"] is False
    assert r["kind"] == "reply"


def test_a_rollback_after_the_start_ends_it_and_a_new_start_counts_again():
    assert drafts.fit(_thread("Deploy started.", "Rolled back, the canary was red."), draft="Can you add a test?")["kind"] == "reply"
    assert drafts.fit(_thread("Deploy started.", "Rolled back.", "Deploying again with the fix."), draft="Can you add a test?")["kind"] == "ack"


@pytest.mark.parametrize(("text", "emoji"), [("thanks", "pray"), ("Thanks!", "pray"), ("ty", "pray"), ("ok", "+1"), ("Done.", "white_check_mark"), ("awesome", "raised_hands"), ("+1", "+1"), (":tada:", "+1")])
def test_a_one_word_thanks_yields_an_emoji_suggestion(text, emoji):
    r = drafts.fit(_thread("Merged your fix.", text), draft="You're welcome, glad it helped!")
    assert (r["kind"], r["emoji"], r["text"], r["keep_draft"]) == ("emoji", emoji, None, False)


def test_a_small_ack_draft_becomes_an_emoji_and_a_long_message_is_not_small():
    assert drafts.fit(_thread("Here is the doc you asked for."), draft="thanks!")["kind"] == "emoji"
    assert drafts.small_ack("thanks for the detailed review of the doc") is None


def test_the_newest_message_decides_and_an_older_draft_is_stale():
    thread = _thread("Can you look at the PR?", "Never mind, I merged it.", "Deploy started.")
    r = drafts.fit(thread, draft="Please split the PR in two.", seen=thread[0]["ts"])
    assert (r["kind"], r["newer"], r["stale"], r["newest_ts"]) == ("ack", 2, True, thread[-1]["ts"])
    plain = drafts.fit(_thread("Can you look at the PR?", "Also the doc."), draft="Looking now.", seen="1700000000.000100")
    assert (plain["kind"], plain["keep_draft"]) == ("reply", False)
    assert "1 newer message(s)" in plain["reason"]


def test_messages_are_read_in_ts_order_and_my_newest_message_needs_no_reply():
    rows = [{"ts": "1700000002.0", "text": "Deploy started.", "mine": False}, {"ts": "1700000001.0", "text": "Can you add a test?", "mine": False}]
    assert drafts.fit(drafts.messages({"messages": rows}))["kind"] == "ack"
    assert drafts.fit(_thread("Can you add a test?", "Added it.", mine_last=True), draft="ok")["kind"] == "none"
    with pytest.raises(drafts.DraftError, match="no messages"):
        drafts.messages({"messages": []})


# Save rules


class KeepsOld(drafts.LocalBackend):
    """A chat tool that silently keeps the old text: save does nothing once a draft exists."""

    def clear(self, target: str) -> None:
        pass

    def save(self, target: str, text: str) -> None:
        if self.read(target) is None:
            super().save(target, text)


class Trims(drafts.LocalBackend):
    """A chat tool that cuts the draft at 10 characters."""

    def save(self, target: str, text: str) -> None:
        super().save(target, text[:10])


def test_replace_clears_saves_and_reads_back_encrypted_at_rest(home):
    be = drafts.LocalBackend()
    be.save("C1/1.0", "old text")
    r = drafts.replace("C1/1.0", "Looks good, approved.\r\n", be)
    assert (r["same"], r["sent_chars"]) == (True, len("Looks good, approved."))
    assert be.read("C1/1.0") == "Looks good, approved.\r\n"
    raw = be.path("C1/1.0").read_bytes()
    assert b"approved" not in raw and b"C1" not in be.path("C1/1.0").name.encode()


def test_a_read_back_mismatch_fails_the_save(home):
    with pytest.raises(drafts.DraftMismatch, match="line 1"):
        drafts.replace("C1/2.0", "This reply is longer than ten characters.", Trims())


def test_an_old_draft_that_survives_clearing_fails_before_saving(home):
    be = KeepsOld()
    drafts.LocalBackend().save("C1/3.0", "old text")
    with pytest.raises(drafts.DraftMismatch, match="still there"):
        drafts.replace("C1/3.0", "new text", be)
    assert be.read("C1/3.0") == "old text"


def test_a_second_writer_of_one_target_waits_then_is_refused_with_the_uses_fix(home):
    held, release = threading.Event(), threading.Event()

    def hold():
        with drafts.locked("C1/4.0"):
            held.set()
            release.wait(5)

    t = threading.Thread(target=hold)
    t.start()
    held.wait(5)
    try:
        start = time.monotonic()
        with pytest.raises(drafts.DraftBusy, match="uses: slack-draft:C1/4.0"):
            drafts.replace("C1/4.0", "text", wait=0.2)
        assert time.monotonic() - start >= 0.2
        assert drafts.replace("C1/5.0", "another target is free", wait=0.2)["same"]
    finally:
        release.set()
        t.join()
    assert drafts.replace("C1/4.0", "text", wait=0.2)["same"]


def test_parallel_replaces_of_one_target_never_interleave(home):
    errors: list[Exception] = []

    def write(n: int):
        try:
            drafts.replace("C1/6.0", f"draft number {n}\n" * 50)
        except Exception as e:  # noqa: BLE001
            errors.append(e)

    threads = [threading.Thread(target=write, args=(n,)) for n in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert errors == []
    text = drafts.LocalBackend().read("C1/6.0") or ""
    assert len(set(text.splitlines())) == 1


def test_targets_and_empty_drafts_are_checked():
    assert drafts.uses_entry("C1/1700000000.000100") == "slack-draft:C1/1700000000.000100"
    for bad in ("", "../etc", "a b", "x" * 200):
        with pytest.raises(drafts.DraftError):
            drafts.check_target(bad)
    with pytest.raises(drafts.DraftError, match="empty"):
        drafts.replace("C1/7.0", "  \n")


# CLI and privacy


def test_cli_fit_and_replace_print_no_thread_or_draft_text(tmp_path, capsys, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    thread = tmp_path / "thread.json"
    thread.write_text(json.dumps({"messages": [{"ts": "1.0", "text": f"{CANARIES[0]} asked for {CANARIES[2]}"}, {"ts": "2.0", "text": "Deploy started."}]}))
    draft = tmp_path / "draft.txt"
    draft.write_text(f"Could you change {CANARIES[1]} first?")
    assert cli.main(["draft", "fit", "--thread", str(thread), "--draft", str(draft), "--seen", "1.0", "--json"]) == 0
    r = json.loads(capsys.readouterr().out)
    assert (r["kind"], r["newer"]) == ("ack", 1)
    assert cli.main(["draft", "fit", "--thread", str(thread), "--draft", str(draft)]) == 0
    assert cli.main(["draft", "replace", "C9/2.0", "--file", str(draft)]) == 0
    out = capsys.readouterr().out
    assert "draft: ack" in out and "slack-draft:C9/2.0" in out
    log = events.log_path().read_text()
    saved = drafts.LocalBackend().path("C9/2.0")
    assert CANARIES[1] in (store.read_text(saved) or "")
    for text in (out, log, saved.read_bytes().decode("latin-1")):
        assert not any(c in text for c in CANARIES)
    assert '"event": "draft.replace"' in log and '"status": "ack"' in log


def test_cli_check_fails_on_a_mismatch_and_errors_name_the_story(tmp_path, capsys):
    sent, back = tmp_path / "sent.txt", tmp_path / "back.txt"
    sent.write_text("Looks good.\n")
    back.write_text("Looks good.  \n\n")
    assert cli.main(["draft", "check", "--sent", str(sent), "--read-back", str(back)]) == 0
    back.write_text("Please change the flag.")
    assert cli.main(["draft", "check", "--sent", str(sent), "--read-back", str(back)]) == 1
    assert cli.main(["draft", "fit", "--thread", str(tmp_path / "missing.json")]) == 2
    out = capsys.readouterr().out
    assert "differs at line 1" in out and "breaks US-98" in out and "Please change" not in out
