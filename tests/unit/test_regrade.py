"""Regression tests for the second grader pass (2026-10-19)."""

import json
import socket

import pytest

from careerbrew import changes, interview, prep, server
from careerbrew.day import planner
from careerbrew.work import priorities

from .test_server import api  # noqa: F401  (fixture)


@pytest.mark.parametrize("bad", [float("nan"), float("inf"), -float("inf"), 1e-9, 1e6])
def test_hours_must_be_finite_and_sane(home, bad):
    with pytest.raises(ValueError, match="hours"):
        priorities.add("x", 3, 3, bad, [])
    priorities.add("ok", 3, 3, 1, [])
    with pytest.raises(ValueError, match="hours"):
        priorities.done("t001", bad)


def test_legacy_bad_estimates_cannot_break_planning(home):
    priorities.save([{"id": f"t00{i}", "title": f"legacy {i}", "impact": 3, "urgency": 3, "estimate_h": v, "competencies": [], "status": "open"} for i, v in enumerate([float("nan"), float("inf"), -1, "abc", None], 1)])
    (home / "metrics.jsonl").write_text(json.dumps({"metric": "estimate_ratio", "value": float("nan"), "at": "2026-10-18T10:00:00+00:00"}) + "\n")
    priorities.plan({})
    plan = planner.build(persist=False)
    assert plan["schedule"], "legacy tasks are still planned"
    assert all(b["start"] < b["end"] for b in plan["schedule"]), "no block may run backwards"


def test_unknown_task_is_404_and_names_a_real_command(api):  # noqa: F811
    status, body = api("/api/tasks/t999/done", body={"hours": 1})
    assert status == 404
    assert "careerbrew work tasks" not in body["error"]


def test_nan_in_data_still_sends_valid_json(api, home):  # noqa: F811
    (home / "metrics.jsonl").write_text(json.dumps({"metric": "estimate_ratio", "value": float("nan"), "at": "2026-10-18T10:00:00+00:00"}) + "\n")
    for path in ("/api/work", "/api/review", "/api/today"):
        status, _ = api(path)  # the fixture parses with json.loads, which accepts NaN; check strictly below
        assert status == 200
    srv, _t = server.serve_in_thread(0, "tok")
    try:
        raw = _raw(srv.server_address[1], f"GET /api/review HTTP/1.0\r\nHost: 127.0.0.1:{srv.server_address[1]}\r\nX-Careerbrew-Token: tok\r\n\r\n")
        body = raw.split(b"\r\n\r\n", 1)[1]
        json.loads(body, parse_constant=lambda c: (_ for _ in ()).throw(AssertionError(f"bare {c} in JSON")))
    finally:
        srv.shutdown()
        srv.server_close()


def _raw(port, req: str | bytes) -> bytes:
    with socket.create_connection(("127.0.0.1", port), timeout=5) as s:
        s.sendall(req.encode() if isinstance(req, str) else req)
        s.shutdown(socket.SHUT_WR)
        out = b""
        while chunk := s.recv(65536):
            out += chunk
    return out


def test_hostile_requests_get_a_status_not_a_dropped_connection(home):
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]
    try:
        nested = b"[" * 30000 + b"]" * 30000
        head = f"POST /api/tasks HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nX-Careerbrew-Token: tok\r\nContent-Type: application/json\r\n"
        assert _raw(port, (head + f"Content-Length: {len(nested)}\r\n\r\n").encode() + nested).startswith(b"HTTP/1.0 400")
        assert _raw(port, head + "Content-Length: -1\r\n\r\n").startswith(b"HTTP/1.0 400")
        body = b'{"title": "x", "hours": 1e999}'
        assert _raw(port, (head + f"Content-Length: {len(body)}\r\n\r\n").encode() + body).startswith(b"HTTP/1.0 400")
    finally:
        srv.shutdown()
        srv.server_close()


def test_every_cited_source_maps_back_to_its_company(home):
    amazon = interview.find_company("Amazon")
    cited = set((amazon.get("remote_policy") or {}).get("sources", [])) | {s for t in amazon.get("company_tips", []) for s in t.get("sources", [])}
    assert cited
    for sid in cited:
        assert "Amazon" in changes.companies_for_source(sid), sid


def test_pivot_on_unknown_company_marks_nothing(home):
    changes.pivot({"kind": "company", "id": "acmecorpx", "change": "removed"})
    assert prep.stale_mark("acmecorpx") is None


def test_unseeded_company_guide_includes_shared_ai_guidance(home):
    text = interview.render(interview.loop_for("Zzunknownco"))
    assert "AI tools" in text


def test_empty_company_name_is_refused(home):
    with pytest.raises(ValueError):
        interview.loop_for("  ")


def test_bad_saved_ratios_are_ignored(home):
    from careerbrew import review

    rows = [{"metric": "estimate_ratio", "value": v, "at": "2026-10-18T10:00:00+00:00"} for v in (float("nan"), float("inf"), 0.0, 2.0)]
    home.mkdir(parents=True, exist_ok=True)
    (home / "metrics.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    assert planner.calibration() == 2.0
    assert review.measure()["estimate_ratio"] == 2.0
