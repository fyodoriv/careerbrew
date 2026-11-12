"""Owner-reviewed recovery after a sent promise's evidence changes."""

import json
from copy import deepcopy
from datetime import date
from pathlib import Path

import pytest

from careerbrew import actions, store
from careerbrew.day import pm_learn, pm_promises
from careerbrew.work import projects, promises

FIXTURE = Path(__file__).resolve().parents[1] / "fixtures/pm-promise-outcomes/histories.json"


def seed(monkeypatch):
    data = json.loads(FIXTURE.read_text())
    monkeypatch.setenv("CAREERBREW_NOW", data["clock"])
    pm_learn.save_sent(data["first_day"], data["first"], receipt=data["first_receipt"])
    original = promises.open_list()[0]["name"]
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    proposal = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    pm_promises.accept(proposal)
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert warning["state"] == "decision-needs-review"
    return data, original, warning


def test_corrected_done_does_not_create_a_duplicate_in_the_day_plan(monkeypatch):
    data, original, _ = seed(monkeypatch)
    pm_promises.reconcile(date(2026, 11, 3))
    same_identity = [name for name, row in projects.records().items() if (row.get("promise") or {}).get("identity_digest") == projects.records()[original]["promise"]["identity_digest"]]
    assert same_identity == [original]
    plan = pm_promises.view(date(2026, 11, 3))
    [item] = plan["items"]
    assert item["status"] == "decision-needs-review"
    assert item["review_ids"] == [original]
    assert not item["session_project"]
    assert not plan["needs_recording"]


def test_existing_same_day_promise_is_preserved_during_review(monkeypatch):
    data, original, _ = seed(monkeypatch)
    rows = pm_promises._rows(data["next_day"], data["O02"]["sent"])
    with projects.locked():
        projects.sync_pm_promises(data["next_day"], rows)
    existing = rows[0]["name"]
    assert existing != original
    pm_promises.reconcile(date(2026, 11, 3))
    assert not projects.records()[existing].get("archived_at")
    [item] = pm_promises.view(date(2026, 11, 3))["items"]
    assert item["status"] == "decision-needs-review" and item["review_ids"] == [original]


def test_missing_receipt_keeps_the_plan_in_review(monkeypatch):
    data, original, _ = seed(monkeypatch)
    store.write_json(pm_learn._outcome_path(data["next_day"]), {"revision": "invalid"})
    [item] = pm_promises.public(pm_promises.view(date(2026, 11, 3)))["items"]
    assert item["status"] == "decision-needs-review" and item["review_ids"] == [original]
    assert "title" not in item and "session_project" not in item


@pytest.mark.parametrize("choice", ["keep", "reopen", "drop"])
@pytest.mark.parametrize("existing", [False, True])
def test_resolution_explicitly_creates_or_keeps_a_separate_promise(choice, existing, monkeypatch):
    data, original, _ = seed(monkeypatch)
    [candidate] = pm_promises._rows(data["next_day"], data["O02"]["sent"])
    if existing:
        with projects.locked():
            projects.sync_pm_promises(data["next_day"], [candidate])
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    option = next(row for row in warning["resolutions"] if row["choice"] == choice)
    assert option["separate_due"] == "2026-11-03"
    assert option["separate_existing"] is existing
    assert actions.run("work.promise.accept", {"proposal_id": option["id"]}, confirm=True)["exit"] == 0
    records = projects.records()
    assert candidate["name"] in records and not records[candidate["name"]].get("archived_at")
    assert records[original]["promise"]["pm_decision_history"][-1]["state"] == "done"
    assert actions.run("work.promise.accept", {"proposal_id": option["id"]}, confirm=True)["exit"] == 2


@pytest.mark.parametrize("archived", [False, True])
def test_competing_prior_identity_blocks_combined_choice(archived, monkeypatch):
    _, original, warning = seed(monkeypatch)
    stale = warning["resolutions"][0]["id"]
    with projects.locked():
        records = projects.records()
        other = deepcopy(records[original])
        other["promise"]["pm_day"] = "2026-10-29"
        other["promise"].pop("pm_decision", None)
        if not archived:
            other.pop("archived_at", None)
            other["promise"].pop("closed", None)
        records["promise-12345678"] = other
        projects._save(records)
    before = projects.path().read_bytes()
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert warning["resolutions"] == []
    assert actions.run("work.promise.accept", {"proposal_id": stale}, confirm=True)["exit"] == 2
    assert projects.path().read_bytes() == before


@pytest.mark.parametrize("defect", ["removed", "closed", "kind", "due"])
def test_invalid_existing_candidate_blocks_combined_choice(defect, monkeypatch):
    data, original, _ = seed(monkeypatch)
    [candidate] = pm_promises._rows(data["next_day"], data["O02"]["sent"])
    with projects.locked():
        projects.sync_pm_promises(data["next_day"], [candidate])
        records = projects.records()
        row = records[candidate["name"]]
        if defect == "removed":
            row["promise"]["pm_removed"] = True
        elif defect == "closed":
            row["promise"]["closed"] = "done"
        elif defect == "kind":
            row["kind"] = "project"
        else:
            row["promise"]["due"] = "2026-11-04"
        projects._save(records)
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert warning["resolutions"] == []


@pytest.mark.parametrize("choice", ["keep", "reopen", "drop"])
def test_owner_resolves_changed_decision_once(choice, monkeypatch):
    data, original, warning = seed(monkeypatch)
    options = {row["choice"]: row for row in warning["resolutions"]}
    assert set(options) == {"keep", "reopen", "drop"}
    assert options["reopen"]["due"] == "2026-11-02"
    proposal = options[choice]["id"]
    before = projects.path().read_bytes()
    with pytest.raises(actions.ActionError, match="confirm to run"):
        actions.run("work.promise.accept", {"proposal_id": proposal})
    assert projects.path().read_bytes() == before
    assert actions.run("work.promise.accept", {"proposal_id": proposal}, confirm=True)["exit"] == 0
    applied = projects.records()[original]
    assert applied["promise"]["pm_decision_history"][-1]["state"] == "done"
    assert not applied["promise"].get("pm_decision")
    if choice == "keep":
        assert applied["promise"]["closed"] == "done" and applied.get("archived_at")
        assert promises.stats(7)["kept_on_time"] == 1
    elif choice == "reopen":
        assert not applied.get("archived_at") and not applied["promise"].get("closed")
        assert applied["promise"]["due"] == "2026-11-02"
        assert promises.stats(7)["kept_on_time"] == 0
    else:
        assert applied["promise"]["closed"] == "dropped" and applied.get("archived_at")
        assert promises.stats(7)["dropped"] == 1
    saved = projects.path().read_bytes()
    assert actions.run("work.promise.accept", {"proposal_id": proposal}, confirm=True)["exit"] == 2
    assert projects.path().read_bytes() == saved
    assert not any(row["state"] == "decision-needs-review" for row in pm_promises.outcomes() if row["promise"] == original)


@pytest.mark.parametrize("change", ["source", "record", "record_metadata", "binding"])
def test_resolution_refuses_stale_inputs_without_mutation(change, monkeypatch):
    data, original, warning = seed(monkeypatch)
    proposal = warning["resolutions"][0]["id"]
    if change == "source":
        pm_learn.save_sent(data["next_day"], data["O03"]["sent"], receipt=data["next_receipt"])
    elif change == "record":
        projects.promise_step(original, "status", "Synthetic progress")
    elif change == "record_metadata":
        projects.edit(original, title="Synthetic corrected title")
    else:
        store.write_json(pm_learn._outcome_path(data["next_day"]), {"revision": "invalid"})
    before = projects.path().read_bytes()
    assert actions.run("work.promise.accept", {"proposal_id": proposal}, confirm=True)["exit"] == 2
    assert projects.path().read_bytes() == before


def test_keep_preserves_observed_completion_day_when_accepted_later(monkeypatch):
    data = json.loads(FIXTURE.read_text())
    monkeypatch.setenv("CAREERBREW_NOW", data["clock"])
    pm_learn.save_sent(data["first_day"], data["first"], receipt=data["first_receipt"])
    original = promises.open_list()[0]["name"]
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    proposal = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-03T12:00:00-04:00")
    pm_promises.accept(proposal)
    assert promises.stats(7)["kept_on_time"] == 1
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-04T12:00:00-04:00")
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    keep = next(row["id"] for row in warning["resolutions"] if row["choice"] == "keep")
    assert actions.run("work.promise.accept", {"proposal_id": keep}, confirm=True)["exit"] == 0
    assert promises.stats(7)["kept_on_time"] == 1
    assert promises.stats(7)["kept_late"] == 0


@pytest.mark.parametrize("manual", ["reopen", "drop"])
def test_keep_reapplies_prior_decision_after_later_manual_change(manual, monkeypatch):
    _, original, _ = seed(monkeypatch)
    if manual == "reopen":
        projects.archive(original, on=False)
    else:
        promises.close(original, "dropped")
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert warning["prior_state"] == "done"
    assert warning["current_state"] == ("open" if manual == "reopen" else "dropped")
    keep = next(row["id"] for row in warning["resolutions"] if row["choice"] == "keep")
    assert actions.run("work.promise.accept", {"proposal_id": keep}, confirm=True)["exit"] == 0
    row = projects.records()[original]
    assert row.get("archived_at") and row["promise"]["closed"] == "done"
    assert promises.stats(7)["kept_on_time"] == 1


def test_keep_reapplies_a_moved_decision_after_manual_drop(monkeypatch):
    data = json.loads(FIXTURE.read_text())
    monkeypatch.setenv("CAREERBREW_NOW", data["clock"])
    pm_learn.save_sent(data["first_day"], data["first"], receipt=data["first_receipt"])
    original = promises.open_list()[0]["name"]
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    proposal = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    pm_promises.accept(proposal)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    promises.close(original, "dropped")
    with projects.locked():
        records = projects.records()
        records[original]["promise"]["due"] = "2026-11-05"
        projects._save(records)
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert (warning["prior_state"], warning["current_state"]) == ("moved", "dropped")
    assert (warning["prior_due"], warning["current_due"]) == ("2026-11-03", "2026-11-05")
    keep_option = next(row for row in warning["resolutions"] if row["choice"] == "keep")
    assert keep_option["due"] == "2026-11-03"
    keep = keep_option["id"]
    assert actions.run("work.promise.accept", {"proposal_id": keep}, confirm=True)["exit"] == 0
    row = projects.records()[original]
    assert not row.get("archived_at") and not row["promise"].get("closed")
    assert row["promise"]["due"] == "2026-11-03"
    assert promises.stats(7)["moved"] == 1


def test_invalid_saved_states_are_not_echoed_or_actionable(monkeypatch):
    _, original, _ = seed(monkeypatch)
    with projects.locked():
        records = projects.records()
        records[original]["promise"]["pm_decision"]["state"] = "synthetic-private-canary"
        records[original]["promise"]["closed"] = "synthetic-private-canary"
        projects._save(records)
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert "synthetic-private-canary" not in json.dumps(warning)
    assert warning["prior_state"] == "unknown" and warning["current_state"] == "archived"
    assert warning["resolutions"] == []


def test_invalid_saved_due_is_not_echoed_or_actionable(monkeypatch):
    _, original, _ = seed(monkeypatch)
    with projects.locked():
        records = projects.records()
        records[original]["promise"]["original_due"] = "synthetic-private-canary"
        projects._save(records)
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert "synthetic-private-canary" not in json.dumps(warning)
    assert warning["resolutions"] == []


def test_forged_resolution_with_invalid_day_does_not_expose_it(monkeypatch):
    _, original, _ = seed(monkeypatch)
    with projects.locked():
        records = projects.records()
        records[original]["promise"]["pm_decision"]["day"] = "synthetic-private-canary"
        projects._save(records)
    with pytest.raises(projects.ProjectError, match="no longer current") as error:
        pm_promises.accept("pm-resolution-forged")
    assert "synthetic-private-canary" not in str(error.value)


@pytest.mark.parametrize("field", ["day", "accepted_at"])
def test_invalid_prior_decision_is_not_actionable(field, monkeypatch):
    _, original, _ = seed(monkeypatch)
    with projects.locked():
        records = projects.records()
        records[original]["promise"]["pm_decision"][field] = "synthetic-private-canary"
        projects._save(records)
    warning = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert "synthetic-private-canary" not in json.dumps(warning)
    assert warning["resolutions"] == []
