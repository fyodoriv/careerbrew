"""docs/telemetry-tiles.md stays true to the code: its command list equals telemetry.USER_COMMANDS,
every tile reads only events careerbrew sends, and the retention tile never counts a missing match."""

from __future__ import annotations

import re
from pathlib import Path

from careerbrew import telemetry

DOC = (Path(__file__).resolve().parents[2] / "docs" / "telemetry-tiles.md").read_text(encoding="utf-8")
QUERIES = re.findall(r"```sql\n(.*?)```", DOC, re.S)


def test_the_documented_command_list_equals_the_code():
    section = DOC.split("## User activity: the fixed command list", 1)[1].split("\n## ", 1)[0]
    listed = set(re.findall(r"`([a-z.]+)`", section.split("The list is", 1)[1].split("The queries below", 1)[0]))
    assert listed - {"cli.run", "command", "web.api", "telemetry.send"} == set(telemetry.USER_COMMANDS)


def test_every_tile_query_reads_only_events_careerbrew_sends():
    assert len(QUERIES) == 14
    for q in QUERIES:
        names = re.findall(r"event (?:=|IN \() ?'([a-z_.]+)'", q) + re.findall(r", '([a-z_.]+)'\)", q)
        assert names and set(names) <= telemetry.EVENT_NAMES, q


def test_the_retention_tile_never_counts_a_missing_match():
    retention = next(q for q in QUERIES if "LEFT JOIN" in q)
    assert "count(DISTINCT w2.distinct_id)" not in retention and "nullIf(w2.distinct_id, '')" in retention


def test_the_work_day_tile_reads_the_counts_day_end_sends():
    from careerbrew.workday import end

    tile = DOC.split("## Work days by version", 1)[1].split("\n## ", 1)[0]
    read = set(re.findall(r"properties\.(steps_[a-z]+|needs_you)", tile))
    assert read and read <= set(end.step_outcomes([]))


def test_the_web_controls_tile_reads_the_act_event():
    tile = DOC.split("## Web controls used per week", 1)[1].split("\n## ", 1)[0]
    assert "event = 'web.act'" in tile and "web.act" in telemetry.EVENT_NAMES
