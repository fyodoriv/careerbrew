"""Synthetic-only claim measurement preparation (#358); no admission enforcement.

Lifecycle: each invocation owns its immutable fixture inputs and capture bundle.
This module imports no application writer, reads no configured data root, invokes
no model and changes no rendering. Candidate text never supplies its own truth.
The finite proposition oracle covers the committed corpus, not arbitrary prose.
"""

from __future__ import annotations

import copy
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import unquote

REPOSITORY = Path(__file__).resolve().parents[3]
FIXTURE_ROOT = REPOSITORY / "tests" / "fixtures" / "claims"
SOURCE_CANARY = "CANARY-CLAIM-SOURCE-358"
FROZEN_FILES = {
    "manifest.json": "235cfc814d33e796756f2fc5da3509108d5d4326a2d37696dab9a31336709d2d",
    "truth/records.json": "d01e1cff1a0e1cb5aaaef2387093811ebef1d53af7ce00203b40bc809ecfc78d",
    "candidates/records.json": "ba4c22c8b110d43ac8dafef5014de0b235eae7b3f3e735b19ebf6a882eab843a",
}
PRESERVATION_IDS = ("P01", "P02", "P03", "P04")
UNSUPPORTED = {
    "shipment-to-users": r"\b(?:shipped|released) (?:the )?parser(?: to users)?\b",
    "sole-ownership": r"\bsole owner\b",
    "production-shipment": r"\bshipped the production parser\b",
    "ownership": r"\b(?:as its owner|owned (?:the )?parser)\b",
    "whole-product-ownership": r"\bowned the complete widget product\b",
    "sole-credit": r"\bsolely delivered the parser\b",
    "co-owner-omission": r"\bsolely delivered the parser\b",
    "user-part-numeric-attribution": r"\b(?:my )?parser reduced product cost by 30\s*%",
    "unscoped-speed-metric": r"\bmade it 3\s*x faster\b",
}
SUPPORTED = {
    "F01": (r"\breleased parser to users\b", r"\bas its owner\b"),
    "F03": (r"\bin review\b", r"\bcontributed parser work with Sam\b"),
    "F05": (r"\bresearch\b", r"\bled parser experiments with Sam\b"),
    "F11": (r"\bled parser work with Sam\b", r"\bparser CPU fell 5\s*%", r"\bon the fixture\b"),
}


class AuditError(ValueError):
    """An integrity refusal named by code; raw source text is never an error."""


@dataclass(frozen=True)
class Inputs:
    manifest: dict[str, Any]
    truth: dict[str, dict[str, Any]]
    candidates: dict[str, dict[str, Any]]
    file_hashes: dict[str, str]


def encoded(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def digest(value: Any) -> str:
    return hashlib.sha256(encoded(value)).hexdigest()


def load_inputs(manifest: Path) -> Inputs:
    if manifest.resolve() != (FIXTURE_ROOT / "manifest.json").resolve():
        raise AuditError("unregistered-fixture-path")
    documents = {}
    hashes = {}
    for name, expected in FROZEN_FILES.items():
        path = FIXTURE_ROOT / name
        if path.is_symlink():
            raise AuditError("fixture-symlink")
        raw = path.read_bytes()
        hashes[name] = hashlib.sha256(raw).hexdigest()
        if hashes[name] != expected:
            raise AuditError("fixture-digest-mismatch")
        documents[name] = json.loads(raw)
    manifest_data = documents["manifest.json"]
    truth = {row["id"]: row for row in documents["truth/records.json"]["records"]}
    candidates = {row["id"]: row for row in documents["candidates/records.json"]["records"]}
    fixture_ids = [f"F{n:02}" for n in range(1, 13)]
    expected_ids = [f"{fixture}.E{n:02}" for fixture in fixture_ids for n in range(1, 8)]
    if list(truth) != fixture_ids or list(candidates) != fixture_ids or manifest_data["derived_attempt_ids"] != expected_ids:
        raise AuditError("fixture-identity-mismatch")
    if any(set(row) != {"id", "clause"} for row in candidates.values()):
        raise AuditError("candidate-truth-field")
    if [key for key, row in truth.items() if row["candidate_supported"]] != manifest_data["supported_fixture_ids"]:
        raise AuditError("independent-truth-mismatch")
    return Inputs(manifest_data, truth, candidates, hashes)


def _asserted_lines(bodies: list[str]) -> str:
    """Inspect assertions, not URL numerals or explicitly quoted refusal text."""
    lines = []
    for body in bodies:
        for raw in body.splitlines():
            line = re.sub(r"^(?:>\s*)+", "", raw.strip())
            if re.match(r"(?:[-*]\s*)?(?:unsupported|not supported|refused|cannot verify|do not claim)\s*:\s*\S", line, re.I):
                continue
            line = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", line)
            line = re.sub(r"<https?://[^|>]+\|([^>]+)>", r"\1", line)
            lines.append(re.sub(r"https?://\S+", "", unquote(line)))
    return " ".join(" ".join(lines).split())


def classify(truth: dict[str, Any], bodies: list[str]) -> dict[str, Any]:
    text = _asserted_lines(bodies)
    unsupported = [name for name in truth["unsupported_propositions"] if re.search(UNSUPPORTED[name], text, re.I)]
    positive = bool(truth["candidate_supported"])
    preserved = positive and all(re.search(pattern, text, re.I) for pattern in SUPPORTED[truth["id"]])
    return {"unsafe": bool(unsupported), "unsupported_propositions": unsupported, "supported_candidate": positive, "safe_preserved": bool(preserved and not unsupported)}


def _captured(capture: dict[str, Any], exit_id: str) -> bool:
    return capture["state"] == "captured" and capture["invoked"] is True and capture["return_status"] == (2 if exit_id == "E04" else 0)


def observe(inputs: Inputs, captures: dict[str, Any], *, provenance: str) -> dict[str, Any]:
    """Read captured facts only. Never call a renderer or rewrite source records."""
    records = []
    for ident in inputs.manifest["derived_attempt_ids"]:
        fixture, exit_id = ident.split(".")
        pair = captures[ident]
        current, control = pair["observed"], pair["control"]
        observed = _captured(current, exit_id)
        result = classify(inputs.truth[fixture], current["bodies"])
        clause = " ".join(inputs.candidates[fixture]["clause"].split()).rstrip(".")
        candidate_present = bool(re.search(re.escape(clause), _asserted_lines(current["bodies"]), re.I))
        records.append(
            {
                "id": ident,
                "kind": "derived",
                "fixture": fixture,
                "exit": exit_id,
                "source_version": inputs.manifest["registration"]["source_version"],
                "truth_digest": digest(inputs.truth[fixture]),
                "capture_digest": digest(pair),
                "state": current["state"],
                "invoked": current["invoked"],
                "return_status": current["return_status"],
                "bodies": copy.deepcopy(current["bodies"]),
                "output_digest": digest(current["bodies"]),
                "stdout": current["stdout"],
                "stderr": current["stderr"],
                "diagnostics": copy.deepcopy(current["diagnostics"]),
                "observed": observed,
                "candidate_clause_present": candidate_present,
                "presentation_outcome": "runtime-error" if not observed else "captured" if candidate_present else "omission",
                "output_unchanged": _captured(control, exit_id) and current["bodies"] == control["bodies"],
                **result,
            }
        )
    for ident in PRESERVATION_IDS:
        records.append({**copy.deepcopy(captures[ident]), "capture_digest": digest(captures[ident])})
    records.append({"id": "X01", "kind": "unsupported", "state": "unsupported-current-surface", "invoked": False, "output": None})
    return {"schema_version": 1, "provenance": provenance, "fixture_hashes": dict(inputs.file_hashes), "records": records}


def inspect_bundle(inputs: Inputs, bundle: dict[str, Any], captures: dict[str, Any], *, allow_planted: bool = False) -> dict[str, Any]:
    """Cross-check observations against a separately retained capture ledger."""
    provenance = bundle.get("provenance")
    if provenance != "real-handler" and not (allow_planted and provenance in {"planted-unit-only", "planted-integrity-only"}):
        raise AuditError("planted-capture-not-baseline")
    expected_ids = [*inputs.manifest["derived_attempt_ids"], *PRESERVATION_IDS, "X01"]
    rows = bundle.get("records", [])
    ids = [row.get("id") for row in rows]
    if len(ids) != 89 or len(set(ids)) != 89 or set(ids) != set(expected_ids):
        raise AuditError("observation-identities")
    if set(captures) != set(expected_ids) - {"X01"}:
        raise AuditError("capture-identities")
    for ident, entry in captures.items():
        values = (entry["control"], entry["observed"]) if "." in ident else (entry,)
        for value in values:
            if value.get("provenance") != provenance or value.get("id") != ident:
                raise AuditError("capture-provenance")
            if value.get("violations"):
                raise AuditError("isolation-violation")
            if SOURCE_CANARY in json.dumps(value.get("diagnostics", {}), ensure_ascii=False):
                raise AuditError("forbidden-data-exit")
    expected = observe(inputs, captures, provenance=provenance)
    if bundle != expected:
        raise AuditError("capture-truth-or-observation-mismatch")
    derived = [row for row in rows if row["kind"] == "derived"]
    negatives = [row for row in derived if not row["supported_candidate"]]
    positives = [row for row in derived if row["supported_candidate"]]
    if len(negatives) != 56 or len(positives) != 28:
        raise AuditError("denominator-mismatch")
    missing_negative = sum(not row["observed"] for row in negatives)
    unsafe = sum(row["observed"] and row["unsafe"] for row in negatives)
    safe_not_preserved = sum(not row["observed"] or not row["safe_preserved"] for row in positives)
    complete = all(row["observed"] for row in derived)
    byte_changes = sum(not row["output_unchanged"] for row in derived)
    preserved = all(captures[ident]["state"] == "captured" and bool(captures[ident]["before"]) and captures[ident]["before"] == captures[ident]["after"] for ident in PRESERVATION_IDS)
    return {
        "integrity_accepted": True,
        "measurement_kind": provenance,
        "records": 89,
        "derived_attempts": 84,
        "negative_denominator": 56,
        "positive_denominator": 28,
        "unsafe_emitted": unsafe,
        "unobserved_negative": missing_negative,
        "safe_not_preserved": safe_not_preserved,
        "observed_positive_count": sum(row["observed"] for row in positives),
        "coverage_complete": complete,
        "generated_output_changes": byte_changes,
        "owner_sources_preserved": preserved,
        "baseline_B": unsafe if provenance == "real-handler" and complete else None,
        "preparation_pass": provenance == "real-handler" and complete and not byte_changes and preserved,
        "unsupported_current_surfaces": 1,
        "future_adapter_observations_executed": 0,
    }


def perturb(bundle: dict[str, Any], control: str, inputs: Inputs) -> dict[str, Any]:
    """Registered planted integrity controls, separate from handler measurements."""
    changed = copy.deepcopy(bundle)
    rows = changed["records"]
    by_id = {row["id"]: row for row in rows}
    if control == "N01":
        rows.remove(by_id["F01.E01"])
    elif control == "N02":
        rows.append(copy.deepcopy(by_id["F01.E01"]))
    elif control == "N03":
        by_id["F01.E01"].update(id="F01.E99", exit="E99")
    elif control == "N04":
        rows.append({"id": "F99.E01", "kind": "derived", "truth": {"id": "F99"}})
    elif control == "N05":
        by_id["F02.E01"]["truth_digest"] = digest({"source_version": "stale"})
    elif control == "N06":
        by_id["F02.E01"]["supported_candidate"] = True
    elif control == "N07":
        row = by_id["F08.E01"]
        row.update(bodies=[inputs.candidates["F08"]["clause"]], unsafe=False)
    elif control == "N08":
        row = by_id["F08.E01"]
        row["bodies"] = []
        row["output_digest"] = digest([inputs.candidates["F08"]["clause"]])
    elif control == "N09":
        by_id["F01.E01"]["diagnostics"]["errors"] = [SOURCE_CANARY]
    elif control == "N10":
        by_id["X01"].update(state="captured", invoked=True, admitted=True, output="fabricated")
    elif control == "N11":
        by_id["P01"]["after"] = {"synthetic-source": {"stored": "changed", "logical": "changed"}}
    elif control == "N12":
        by_id["F02.E01"].update(state="captured", invoked=True, observed=True, return_status=0, bodies=[])
    else:
        raise AuditError("unregistered-perturbation")
    return changed


def integrity_controls(inputs: Inputs, bundle: dict[str, Any], captures: dict[str, Any]) -> dict[str, Any]:
    """Keep planted controls out of the observed baseline and its rates."""
    positive = inspect_bundle(inputs, bundle, captures)["integrity_accepted"]
    results = {}
    for n in range(1, 13):
        control = f"N{n:02}"
        planted = copy.deepcopy(captures)
        for ident, entry in planted.items():
            for value in (entry["control"], entry["observed"]) if "." in ident else (entry,):
                value["provenance"] = "planted-integrity-only"
        if control in {"N07", "N08"}:
            for value in planted["F08.E01"].values():
                value.update(state="captured", invoked=True, return_status=0, bodies=[inputs.candidates["F08"]["clause"]])
        if control == "N12":
            # Explicit planted missing execution; never a claim that the handler failed.
            planted["F02.E01"]["observed"].update(state="error", invoked=False, return_status=None, bodies=[])
        base = observe(inputs, planted, provenance="planted-integrity-only")
        broken = perturb(base, control, inputs)
        try:
            inspect_bundle(inputs, broken, planted, allow_planted=True)
        except AuditError as exc:
            results[control] = {"rejected": True, "reason": str(exc), "provenance": "planted-integrity-only"}
        else:
            results[control] = {"rejected": False, "provenance": "planted-integrity-only"}
    return {"unchanged_bundle_positive_accepted": positive, "invalid_rejected": sum(value["rejected"] for value in results.values()), "invalid_denominator": 12, "controls": results}
