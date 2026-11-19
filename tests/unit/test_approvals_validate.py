"""rust-04: the approval queue refuses malformed records with a fixed, content-free message (deny by default)."""

import pytest

from careerbrew import approvals, store
from careerbrew.cli import main

CANARY = "CANARY-7f3a-synthetic-secret"
OK = {"id": "a1", "title": "Open a PR", "at": "2026-11-01T09:00:00+00:00", "status": "pending", "note": ""}
DONE = {"id": "a2", "title": "Ship it", "at": "2026-11-01T09:00:00+00:00", "status": "rejected", "note": "ask first", "decided_at": "2026-11-02T09:00:00+00:00"}

BAD = [
    ([CANARY], "approvals record 0: not an object"),
    ([{**OK, "id": None, "detail": CANARY}], "approvals record 0: missing id"),
    ([{k: v for k, v in OK.items() if k != "status"} | {"detail": CANARY}], "approvals record 0: missing status"),
    ([{**OK, "status": CANARY}], "approvals record 0: unknown status"),
    ([{k: v for k, v in OK.items() if k != "title"} | {"detail": CANARY}], "approvals record 0: missing title"),
    ([{k: v for k, v in OK.items() if k != "at"} | {"detail": CANARY}], "approvals record 0: missing at"),
    ([{k: v for k, v in DONE.items() if k != "decided_at"} | {"note": CANARY}], "approvals record 0: decided without decided_at"),
    ([OK, {**OK, "title": CANARY}], "approvals record 1: duplicate id"),
]


@pytest.mark.parametrize(("records", "message"), BAD)
def test_each_malformed_shape_is_refused_with_a_fixed_message(records, message):
    with pytest.raises(approvals.ApprovalRecordError) as e:
        approvals.validate_records(records)
    assert str(e.value) == message and CANARY not in str(e.value)


def test_a_non_list_file_is_refused():
    with pytest.raises(approvals.ApprovalRecordError, match=r"^approvals file: not a list$"):
        approvals.validate_records({"id": CANARY})


@pytest.mark.parametrize(("records", "message"), BAD)
def test_every_reader_refuses_instead_of_raising_key_error(records, message):
    store.write_json(approvals._path(), records)
    for call in (approvals.pending, approvals.guidance, lambda: approvals.get("a1"), lambda: approvals.decide("a1", "approved")):
        with pytest.raises(approvals.ApprovalRecordError, match=f"^{message}$"):
            call()


@pytest.mark.parametrize(("records", "message"), BAD)
def test_a_request_refuses_a_malformed_file_before_it_writes(records, message):
    store.write_json(approvals._path(), records)
    with pytest.raises(approvals.ApprovalRecordError, match=f"^{message}$"):
        approvals.request("a new ask")
    assert store.read_json(approvals._path(), None) == records


def test_the_cli_prints_the_fixed_message_and_exits_nonzero(capsys):
    store.write_json(approvals._path(), [{**OK, "status": CANARY}])
    assert main(["approvals", "show", "a1"]) == 2
    out = capsys.readouterr()
    assert "error: approvals record 0: unknown status" in out.out + out.err
    assert CANARY not in out.out + out.err and "Traceback" not in out.err


def test_valid_records_behave_as_before():
    store.write_json(approvals._path(), [OK, DONE])
    approvals.validate_records([OK, DONE])
    assert approvals.pending() == [OK]
    assert approvals.guidance() == [{"status": "rejected", "title": "Ship it", "note": "ask first", "at": "2026-11-02T09:00:00+00:00"}]
    assert approvals.get("a2") == DONE
    assert approvals.decide("a1", "approved")["status"] == "approved"
    with pytest.raises(KeyError):
        approvals.get("zz")


def test_an_empty_queue_is_valid():
    assert approvals.pending() == [] and approvals.guidance() == []
