"""The README Proof section is generated from docs/proof/metrics.json, and every metric names its source."""

import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("proof", ROOT / "scripts/proof.py")
assert spec and spec.loader
proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proof)


def test_the_readme_proof_section_matches_the_generator_output():
    assert proof.check() == []


def test_a_metric_without_a_date_kind_or_command_is_a_problem():
    data = {"metrics": {"unit_tests": {"value": 5, "unit": "unit tests", "date": "", "kind": None}}}
    assert proof.problems(data) == ["unit_tests: no date", "unit_tests: no kind", "unit_tests: no command"]


def test_a_headline_row_with_no_field_data_reads_not_measured_yet_never_a_test_number():
    data = {"metrics": {"field_recording_since": {"value": "2026-10-26"}, "unit_tests": {"value": 1345, "unit": "unit tests"}}}
    table = proof.section(data).split("**Engineering health**")[0]
    assert table.count("not measured yet (field recording on since 2026-10-26)") == len(proof.FIELD_ROWS)
    assert "1345" not in table


def test_the_committed_proof_holds_no_machines_real_use(monkeypatch):
    """Field totals are opt-in (PROOF_FIELD=1, for your own copy): the committed file and the README carry none."""
    monkeypatch.delenv("PROOF_FIELD", raising=False)  # an exported opt-in must not change this test
    proof = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(proof)
    data = json.loads((ROOT / "docs/proof/metrics.json").read_text())
    assert not [name for name in data["metrics"] if name.startswith("field_")]
    table = proof.section(data).split("**Engineering health**")[0]
    assert table.count("your own number: run `careerbrew metrics field`") == len(proof.FIELD_ROWS)
    assert "Machines so far" not in table and "field recording on since" not in table.lower()
    assert proof.FIELD is False


def test_the_metrics_file_holds_counts_and_rates_only():
    data = json.loads((ROOT / "docs/proof/metrics.json").read_text())
    for name, value in data["metrics"].items():
        assert value["history"], name
        # a field metric with no real runs yet is None: it renders as `not measured yet`, never a number
        assert isinstance(value["value"], (int, float, str)) or (value["value"] is None and name.startswith("field_")), name
        assert "private/" not in json.dumps(value), name
