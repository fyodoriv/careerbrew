"""Preparation integrity controls; planted captures are never baseline results."""

from copy import deepcopy
from pathlib import Path

import pytest

from careerbrew.work import claim_audit as audit

FIXTURES = Path(__file__).resolve().parents[1] / "fixtures" / "claims" / "manifest.json"


@pytest.fixture
def inputs():
    return audit.load_inputs(FIXTURES)


@pytest.fixture
def planted(inputs):
    """Explicit independent audit-integrity input, without renderer invocation."""
    captures = {}
    for ident in inputs.manifest["derived_attempt_ids"]:
        fixture, exit_id = ident.split(".")
        capture = {
            "id": ident,
            "kind": "derived",
            "provenance": "planted-unit-only",
            "fixture": fixture,
            "exit": exit_id,
            "state": "captured",
            "invoked": True,
            "return_status": 2 if exit_id == "E04" else 0,
            "bodies": [inputs.candidates[fixture]["clause"]],
            "stdout": "",
            "stderr": "",
            "diagnostics": {"events": [], "errors": [], "repo_cache": []},
            "violations": [],
        }
        captures[ident] = {"control": deepcopy(capture), "observed": deepcopy(capture)}
    for ident in ("P01", "P02", "P03", "P04"):
        captures[ident] = {
            "id": ident,
            "kind": "preservation",
            "provenance": "planted-unit-only",
            "state": "captured",
            "before": {"synthetic-source": {"stored": "same", "logical": "same"}},
            "after": {"synthetic-source": {"stored": "same", "logical": "same"}},
            "violations": [],
        }
    bundle = audit.observe(inputs, captures, provenance="planted-unit-only")
    return bundle, captures


def test_complete_unchanged_bundle_is_one_integrity_positive_not_baseline(inputs, planted):
    bundle, captures = planted
    result = audit.inspect_bundle(inputs, bundle, captures, allow_planted=True)
    assert result["integrity_accepted"]
    assert result["records"] == 89
    assert result["derived_attempts"] == 84
    assert result["negative_denominator"] == 56
    assert result["positive_denominator"] == 28
    assert result["measurement_kind"] == "planted-unit-only"
    assert result["baseline_B"] is None
    assert result["unsafe_emitted"] == 56
    assert result["safe_not_preserved"] == 0
    with pytest.raises(audit.AuditError, match="planted-capture"):
        audit.inspect_bundle(inputs, bundle, captures)


@pytest.mark.parametrize("control", [f"N{n:02}" for n in range(1, 13)])
def test_each_registered_invalid_audit_bundle_is_rejected(inputs, planted, control):
    bundle, captures = planted
    if control == "N12":
        captures["F02.E01"]["observed"].update(state="error", invoked=False, bodies=[], return_status=None)
        bundle = audit.observe(inputs, captures, provenance="planted-unit-only")
    broken = audit.perturb(bundle, control, inputs)
    with pytest.raises(audit.AuditError):
        audit.inspect_bundle(inputs, broken, captures, allow_planted=True)


def test_actual_captured_output_and_independent_truth_own_classification(inputs):
    truth = inputs.truth["F08"]
    assert audit.classify(truth, ["Owned the complete widget product."])["unsafe"]
    assert audit.classify(truth, ["Review unavailable.\n> Owned the complete widget product."])["unsafe"]
    assert not audit.classify(inputs.truth["F12"], ["https://example.com/3x-faster"])["unsafe"]
    assert audit.classify(inputs.truth["F10"], ["my parser reduced product cost by 30% (1 PR)."])["unsafe"]


def test_missing_and_errors_cannot_improve_coverage_or_shrink_denominators(inputs, planted):
    bundle, captures = planted
    for ident in ("F02.E01", "F01.E01"):
        captures[ident]["observed"].update(state="error", invoked=False, bodies=[], return_status=None)
    bundle = audit.observe(inputs, captures, provenance="planted-unit-only")
    result = audit.inspect_bundle(inputs, bundle, captures, allow_planted=True)
    assert result["negative_denominator"] == 56 and result["positive_denominator"] == 28
    assert result["unobserved_negative"] == 1
    assert result["safe_not_preserved"] == 1
    assert not result["coverage_complete"]
    assert not result["preparation_pass"]


def test_observer_only_reads_capture_arguments(inputs, planted):
    _, captures = planted
    before = deepcopy(captures)
    audit.observe(inputs, captures, provenance="planted-unit-only")
    assert captures == before


def test_manifest_keeps_future_contracts_and_absent_one_to_one_separate(inputs):
    assert len(inputs.manifest["derived_attempt_ids"]) == 84
    assert inputs.manifest["unsupported_gaps"][0]["id"] == "X01"
    future = inputs.manifest["future_operational_adapter_controls"]
    assert len(future["cases"]) * len(future["draft_kinds"]) == 36
    assert future["host_attestation"] == "unsupported-blocked"
