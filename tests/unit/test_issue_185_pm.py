"""Issue #185, part 2: the owner's PM update edits change what the next draft writes."""

from careerbrew import signals, store
from careerbrew.cli import main
from careerbrew.day import log as daylog
from careerbrew.day import pm_learn

DRAFT = "billing: export (1 PR); next: retries.\nledger: close the books (1 ticket).\ntooling: lint fixes (2 commits).\nOther: 3 meetings."


def _edit(day, body, raw=DRAFT, shown=None):
    """What the owner saw that day (raw and shown drafts), then what they saved."""
    pm_learn.remember(day, raw, raw if shown is None else shown)
    pm_learn.record(day, body)


def _edits(*bodies):
    for n, body in enumerate(bodies, 1):
        _edit(f"2026-10-{n:02d}", body)


CUT_OTHER = "billing: export (1 PR); next: retries.\nledger: close the books (1 ticket).\ntooling: lint fixes (2 commits)."


def test_a_line_the_owner_cuts_twice_is_left_out(home):
    _edits(CUT_OTHER, CUT_OTHER)
    assert pm_learn.rules()["cut"] == ["Other"]
    assert pm_learn.apply(DRAFT.splitlines(), set()) == DRAFT.splitlines()[:3]


def test_one_cut_is_not_a_rule(home):
    _edits(CUT_OTHER)
    assert pm_learn.rules()["cut"] == [] and pm_learn.apply(DRAFT.splitlines(), set()) == DRAFT.splitlines()


def test_saving_the_shown_draft_again_is_not_more_evidence(home):
    """Review finding: the shown draft already hides the line; saving it with an unrelated edit must not count as a cut."""
    _edits(CUT_OTHER, CUT_OTHER)
    shown = "\n".join(pm_learn.apply(DRAFT.splitlines(), set()))
    for day in ("2026-10-03", "2026-10-04", "2026-10-05"):
        _edit(day, shown + "\nplus a note.", shown=shown)
    _edit("2026-10-06", DRAFT, shown=shown)  # the owner types the hidden line back once: counts double
    assert pm_learn.rules()["cut"] == [], "2 cuts against 2 (one add-back, counted double): no longer a lesson"


def test_a_renamed_label_is_written_as_the_owner_writes_it(home):
    body = DRAFT.replace("tooling:", "Developer tools:")
    _edits(body, body)
    assert pm_learn.rules()["rename"] == {"tooling": "Developer tools"}
    assert "Developer tools: lint fixes (2 commits)." in pm_learn.apply(DRAFT.splitlines(), set())


def test_a_rename_with_a_small_text_edit_is_still_a_rename_and_bold_is_ignored(home):
    body = DRAFT.replace("tooling: lint fixes (2 commits).", "- **Developer tools**: lint fixes (2 commits), on time.")
    _edits(body, body)
    assert pm_learn.rules()["rename"] == {"tooling": "Developer tools"} and pm_learn.rules()["cut"] == []
    bold = DRAFT.replace("billing:", "**billing**:")
    _edits(bold, bold)
    assert "billing" not in pm_learn.rules()["rename"], "bold is not a rename"


def test_a_part_the_owner_cuts_from_a_line_is_dropped(home):
    body = DRAFT.replace("; next: retries", "")
    _edits(body, body)
    assert pm_learn.rules()["parts"] == [["billing", "next"]]
    assert pm_learn.apply(DRAFT.splitlines(), set())[0] == "billing: export (1 PR)."


def test_a_semicolon_inside_a_title_is_not_a_part(home):
    _edits(CUT_OTHER, CUT_OTHER)
    line = "billing: fix a; b and c (1 PR); next: retries."
    assert pm_learn.apply([line], set()) == [line]


def test_unranked_lines_follow_the_owners_order_and_ranked_ones_keep_their_rank(home):
    body = "billing: export (1 PR); next: retries.\ntooling: lint fixes (2 commits).\nledger: close the books (1 ticket).\nOther: 3 meetings."
    _edits(body, body)
    out = pm_learn.apply(DRAFT.splitlines(), {"billing"})
    assert [line.split(":")[0] for line in out] == ["billing", "tooling", "ledger", "Other"]


def test_pm_lines_apply_the_lessons_and_the_raw_draft_ignores_them(home):
    d = {"day": "2026-10-16", "tomorrow": "2026-10-19", "done": {"Shipped": ["Reviewed 4 pull requests"]}, "planned": [], "blockers": []}
    d["details"] = {"done": {"Shipped": [{"kind": "github", "repo": ""}]}}
    raw = daylog.pm_lines(d, learn=False)
    assert raw == ["Other: reviewed 4 PRs."]
    for day in ("2026-10-01", "2026-10-02"):
        _edit(day, "Reviews: reviewed 4 PRs.", raw="\n".join(raw))
    assert daylog.pm_lines(d) == ["Reviews: reviewed 4 PRs."] and daylog.pm_lines(d, learn=False) == raw


def test_the_pm_view_remembers_what_it_showed_and_a_save_never_rebuilds_the_day(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    d = {"day": "2026-10-21", "tomorrow": "2026-10-22", "done": {"Shipped": ["Reviewed 4 pull requests"]}, "planned": [], "blockers": []}
    d["details"] = {"done": {"Shipped": [{"kind": "github", "repo": ""}]}}
    pm = daylog.pm_report(d)
    assert store.read_json(pm_learn.shown_path("2026-10-21"))["shown"] == pm["draft"]
    monkeypatch.setattr(daylog, "build", lambda *a, **k: (_ for _ in ()).throw(AssertionError("save rebuilt the day")))
    daylog.save_pm_draft("2026-10-21", "My own words.")
    (row,) = store.read_jsonl(pm_learn.path())
    assert row["body"] == "My own words." and row["shown"] == pm["draft"]
    daylog.save_pm_draft("2026-10-21", "")
    assert store.read_jsonl(pm_learn.path()) == []


def test_the_pm_view_names_lines_the_lessons_leave_out(home):
    _edits(CUT_OTHER, CUT_OTHER)
    d = {"day": "2026-10-16", "tomorrow": "2026-10-19", "done": {"Shipped": ["Reviewed 4 pull requests"]}, "planned": [], "blockers": []}
    d["details"] = {"done": {"Shipped": [{"kind": "github", "repo": ""}]}}
    pm = daylog.pm_report(d)
    assert "Other:" in pm["body"]
    assert pm["left_out"] == []


def test_reset_brings_a_cut_line_back(home, capsys):
    _edits(CUT_OTHER, CUT_OTHER)
    assert main(["signals", "reset", "cut", "Other"]) == 0
    assert "writes that line as generated again" in capsys.readouterr().out
    assert pm_learn.rules()["cut"] == []


def test_an_edit_saved_after_a_reset_the_same_day_counts(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-02T09:00")
    _edits(CUT_OTHER, CUT_OTHER)
    pm_learn.reset("Other")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-02T15:00")
    _edit("2026-10-02", CUT_OTHER)
    _edit("2026-10-03", CUT_OTHER)
    assert pm_learn.rules()["cut"] == ["Other"], "the 15:00 edit came after the 09:00 reset"


def test_a_reset_stored_as_a_date_still_skips_older_rows(home):
    _edits(CUT_OTHER, CUT_OTHER)
    store.write_jsonl(pm_learn.path(), [{k: v for k, v in r.items() if k != "saved"} for r in store.read_jsonl(pm_learn.path())])
    store.write_json(pm_learn.resets_path(), {"other": "2026-10-02"})
    assert pm_learn.rules()["cut"] == []


def test_signals_name_what_the_pm_edits_changed(home, capsys, monkeypatch):
    body = DRAFT.replace("tooling:", "Developer tools:").replace("\nOther: 3 meetings.", "")
    _edits(body, body)
    rows = [r for r in signals.summary() if r["signal"] == "pm"]
    assert {r["kind"] for r in rows} == {"cut", "rename"}
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)
    assert main(["signals"]) == 0
    out = capsys.readouterr().out
    assert "PM update  the PM update leaves out the Other line: you cut it" in out and "writes tooling as Developer tools" in out
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: False)
    assert main(["signals"]) == 0 and "Other" not in capsys.readouterr().out, "labels are business text: hidden from agents"
