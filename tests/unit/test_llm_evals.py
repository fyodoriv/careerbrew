"""LLM-judged evals (US-66): the judge path, owner labels, and the agreement report, with a fake judge."""

from __future__ import annotations

import json
import urllib.error

import pytest

from careerbrew import judge, paths

from .conftest import CANARIES

FAIL_WORD = "exactly one"  # the fake judge FAILs a criterion that holds these words
MODEL = "fixture-judge:1b"  # the tests' own local model: the owner may change the repo's (TASKS.md llm-evals step 1)
MODELS_YAML = f"""routes:
  - name: local
    provider: ollama
    base_url: http://localhost:11434/v1
    model: {MODEL}
    private_data: true
judge:
  route: local
  answer_route: local
"""


class FakeJudge:
    """An OpenAI-style transport: a fixed synthetic answer; as a judge, PASS unless the criterion holds FAIL_WORD."""

    def __init__(self) -> None:
        self.payloads: list[tuple[dict, str]] = []

    def __call__(self, payload: dict, url: str) -> str:
        self.payloads.append((payload, url))
        prompt = payload["messages"][-1]["content"]
        if "Criterion:" in prompt:
            criterion = prompt.split("Criterion:", 1)[1].split("\n", 1)[0]
            return json.dumps({"verdict": "FAIL" if FAIL_WORD in criterion else "PASS", "evidence": "fake judge"})
        return "<think>plan the reply</think>I would run `careerbrew review` and propose one next step."


def case(cid: str, criterion: str = "Proposes a next step", suite: str = "regression", should: str = "act") -> dict:
    return {"id": cid, "suite": suite, "skill": "start-session", "should": should, "input": f"synthetic request {cid}", "expect": [criterion]}


CASES = [
    {"id": "s-act", "suite": "regression", "skill": "start-session", "should": "act", "input": "start careerbrew", "expect": ["Runs review first", f"Proposes {FAIL_WORD} next step"]},
    {"id": "s-refrain", "suite": "regression", "skill": "start-session", "should": "refrain", "input": "what is 2 + 2?", "expect": ["Answers directly"]},
    {"id": "cli-init", "suite": "regression", "skill": "start-session", "should": "act", "cli": ["init"], "expect_regex": "created"},
]


@pytest.fixture
def evals_env(tmp_path, monkeypatch):
    monkeypatch.delenv("EVALS_LLM_URL", raising=False)
    monkeypatch.setenv("EVALS_LABELS", str(tmp_path / "labels.jsonl"))
    monkeypatch.setenv("EVALS_CACHE", str(tmp_path / "cache"))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-22T10:00")
    config = tmp_path / "config"
    config.mkdir()
    (config / "models.yaml").write_text(MODELS_YAML, encoding="utf-8")
    monkeypatch.setattr(paths, "config_dir", lambda: config)  # never the repo's config/models.yaml
    return tmp_path


def make(fake: FakeJudge) -> judge.Judge:
    return judge.Judge.from_config(transport=fake)


def label_all(j: judge.Judge, cases: list[dict], labels: list[str]) -> list[dict]:
    for c, lab in zip(cases, labels, strict=True):
        judge.add_label(c, j.answer(c), lab)
    rows, problems = judge.read_labels()
    assert problems == []
    return rows


def test_parse_verdict_reads_json_then_a_bare_word_and_ignores_thinking():
    assert judge.parse_verdict('{"verdict": "pass", "evidence": "It runs review."}') == judge.Verdict("PASS", "It runs review.")
    assert judge.parse_verdict('<think>maybe PASS</think>```json\n{"verdict":"FAIL","evidence":"no step"}\n```') == judge.Verdict("FAIL", "no step")
    assert judge.parse_verdict("Verdict: UNKNOWN, the answer is cut off").verdict == "UNKNOWN"
    assert judge.parse_verdict("<think>this would PASS</think>looks fine to me") == judge.Verdict("UNKNOWN", "the judge gave no verdict")


def test_a_case_passes_only_when_every_criterion_passes():
    p, f, u = judge.Verdict("PASS", ""), judge.Verdict("FAIL", ""), judge.Verdict("UNKNOWN", "")
    assert judge.case_verdict([p, p]) == "pass"
    assert judge.case_verdict([p, f, u]) == "fail"
    assert judge.case_verdict([p, u]) == "unknown"
    assert judge.case_verdict([]) == "unknown"


def test_run_answers_and_judges_one_criterion_per_call_on_the_local_route_then_caches(evals_env):
    fake = FakeJudge()
    lines, fails = judge.run(CASES, make(fake), [])
    # 2 LLM cases: 2 answers + 3 criteria; the CLI case is not the judge's
    assert len(fake.payloads) == 5
    judge_prompts = [p["messages"][-1]["content"] for p, _ in fake.payloads if "Criterion:" in p["messages"][-1]["content"]]
    assert len(judge_prompts) == 3 and all(p.count("Criterion:") == 1 for p in judge_prompts)
    assert all("<think>" not in p for p in judge_prompts), "the judge grades the answer, not the model's thinking"
    assert all(url == "http://localhost:11434/v1/chat/completions" for _, url in fake.payloads)
    payload = fake.payloads[0][0]
    assert payload["temperature"] == 0 and payload["max_tokens"] == judge.ANSWER_TOKENS and payload["reasoning_effort"] == "none"
    assert lines[0].startswith("llm: 2 case(s) answered on route local") and "1 pass, 1 fail, 0 unknown" in lines[0]
    assert "  s-act: judged fail (criterion 2: FAIL, fake judge) [judge not trusted yet]" in lines
    assert fails == [], "an uncalibrated judge never fails the run"
    assert lines[-1].startswith("decision: COLLECT: 0/20 labels")

    again = make(fake)
    judge.run(CASES, again, [])
    assert again.calls == 0 and len(fake.payloads) == 5, "a rerun with unchanged inputs calls no model"


@pytest.mark.parametrize(
    ("owner_pass", "decision", "trusted"),
    [(17, "KEEP", True), (15, "CALIBRATE", False), (13, "PIVOT", False)],
)
def test_agreement_with_owner_labels_decides_keep_calibrate_or_pivot(evals_env, owner_pass, decision, trusted):
    fake = FakeJudge()
    j = make(fake)
    cases = [case(f"c{i:02d}") for i in range(20)]  # the fake judge passes every one
    labels = label_all(j, cases, ["pass"] * owner_pass + ["fail"] * (20 - owner_pass))
    a = judge.agreement(j, labels, cases)
    assert (a.n, a.agree, a.trusted) == (20, owner_pass, trusted)
    lines = judge.agreement_lines(a, j)
    assert lines[0] == f"judge agreement: {owner_pass}/20 labels agree ({owner_pass / 20:.0%}) on route local ({MODEL})"
    assert "  disagree  c19: owner fail, judge pass" in lines
    assert lines[-1].startswith(f"decision: {decision}")


def test_fewer_than_20_labels_never_trust_the_judge(evals_env):
    j = make(FakeJudge())
    cases = [case(f"c{i}") for i in range(3)]
    a = judge.agreement(j, label_all(j, cases, ["pass"] * 3), cases)
    assert (a.rate, a.trusted) == (1.0, False)
    assert a.decision().startswith("COLLECT: 3/20 labels")


def test_a_trusted_judge_fails_the_run_on_a_judged_regression_failure_only(evals_env):
    j = make(FakeJudge())
    labeled = [case(f"c{i:02d}") for i in range(20)]
    labels = label_all(j, labeled, ["pass"] * 20)
    cases = [*labeled, case("bad-regression", f"Proposes {FAIL_WORD} step"), case("bad-capability", f"Proposes {FAIL_WORD} step", suite="capability")]
    lines, fails = judge.run(cases, j, labels)
    assert fails == ["bad-regression: judged fail (criterion 1: FAIL, fake judge)"]
    assert "  bad-capability: judged fail (criterion 1: FAIL, fake judge)" in lines, "a capability case may fail"
    assert lines[-1].startswith("decision: KEEP")

    _, untrusted = judge.run(cases, j, labels[:19])
    assert untrusted == []


def test_a_label_is_tied_to_the_answer_the_owner_read_and_to_the_criteria(evals_env):
    j = make(FakeJudge())
    c = case("s-one")
    answer = j.answer(c)
    assert judge.add_label(c, answer, "pass") == 1
    assert judge.add_label(c, answer, "fail") == 1, "labeling again replaces the label"
    path = evals_env / "labels.jsonl"
    row = json.loads(path.read_text(encoding="utf-8"))
    assert row == {"answer": answer, "answer_sha": judge.digest(answer), "case": "s-one", "expect_sha": judge.expect_digest(c), "label": "fail", "labeled": "2026-10-22"}

    path.write_text(json.dumps({**row, "answer": answer + " (edited)"}) + "\n", encoding="utf-8")
    labels, problems = judge.read_labels()
    assert labels == [] and problems == ["labels.jsonl line 1 (s-one): the answer changed after it was labeled; label it again: `evals/run.sh show s-one`"]
    assert judge.read_labels(skip_case="s-one") == ([], []), "show and label s-one may run: labeling it again replaces the line"
    assert judge.add_label(c, answer, "pass") == 1, "the named fix works: label the case again"
    assert judge.read_labels() == ([{**row, "label": "pass"}], [])

    path.write_text("{not json\n" + json.dumps(row) + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="first: labels.jsonl line 1: not JSON"):
        judge.add_label(c, answer, "pass")  # a broken line of no case stays until the owner fixes it; no line is lost
    assert path.read_text(encoding="utf-8").startswith("{not json\n")

    path.write_text(json.dumps(row) + "\n" + "{not json\n" + json.dumps({**row, "label": "maybe"}) + "\n", encoding="utf-8")
    labels, problems = judge.read_labels()
    assert len(labels) == 1 and problems == ["labels.jsonl line 2: not JSON", "labels.jsonl line 3 (s-one): label must be pass or fail"]
    assert judge.check_labels(labels + labels, [c]) == ["labels: s-one is labeled twice; keep one line"]
    assert judge.check_labels(labels, []) == ["labels: s-one is not an LLM-judged case in evals/cases/ (renamed or removed?)"]

    changed = {**c, "expect": ["Proposes a different step"]}
    a = judge.agreement(j, labels, [changed])
    assert (a.n, a.stale) == (0, ["s-one"]), "criteria changed: the label is stale and left out"
    assert judge.next_unlabeled(labels, [changed]) == changed


def test_show_prints_the_case_its_criteria_the_answer_and_the_label_command(evals_env):
    j = make(FakeJudge())
    c = CASES[0]
    answer = j.answer(c)
    text = judge.show_text(c, answer, j, [], CASES)
    assert "case: s-act (skill start-session, should act, suite regression)" in text
    assert f"  2. Proposes {FAIL_WORD} next step" in text
    assert "I would run `careerbrew review`" in text and "<think>" not in text
    assert "label it: evals/run.sh label s-act pass   (or fail)" in text and "labels: 0/20" in text
    assert judge.next_unlabeled([], CASES) == CASES[0], "show starts with the first unlabeled case"
    judge.add_label(c, j.cached_answer(c) or "", "pass")
    labels, _ = judge.read_labels()
    assert "your label: pass (2026-10-22)" in judge.show_text(c, answer, j, labels, CASES)
    assert judge.next_unlabeled(labels, CASES) == CASES[1]


@pytest.mark.parametrize(
    ("error", "fix"),
    [
        (urllib.error.URLError(ConnectionRefusedError(61, "Connection refused")), "nothing answers at http://localhost:11434/v1"),
        (urllib.error.HTTPError("http://localhost:11434/v1/chat/completions", 404, "not found", {}, None), f"`ollama pull {MODEL}`, or name a model you have"),  # type: ignore[arg-type]
        (urllib.error.URLError(TimeoutError("timed out")), "no reply from http://localhost:11434/v1 within 300 s"),
        (KeyError("choices"), "not an OpenAI-style chat completion"),
    ],
)
def test_a_judge_that_does_not_answer_names_the_fix(evals_env, error, fix):
    def broken(payload, url):
        raise error

    with pytest.raises(judge.LLMError, match="route local") as e:
        judge.run(CASES, make(broken), [])
    assert fix in str(e.value)


def test_evals_llm_url_moves_both_routes_but_only_on_this_machine(evals_env, monkeypatch):
    fake = FakeJudge()
    monkeypatch.setenv("EVALS_LLM_URL", "http://127.0.0.1:9999/v1")
    judge.run(CASES[:1], make(fake), [])
    assert {url for _, url in fake.payloads} == {"http://127.0.0.1:9999/v1/chat/completions"}
    monkeypatch.setenv("EVALS_LLM_URL", "https://judge.example.com/v1")
    with pytest.raises(judge.LLMError, match="must be on this machine"):
        make(fake)


def test_model_payloads_hold_repo_text_only_never_data_root_canaries(evals_env, ats_sources):
    fake = FakeJudge()
    j = make(fake)
    cases = [c for c in CASES if "cli" not in c]
    labels = label_all(j, cases, ["pass", "fail"])
    judge.run(CASES, j, labels)
    sent = json.dumps([p for p, _ in fake.payloads])
    assert fake.payloads and not any(canary in sent for canary in CANARIES)
    assert not any(canary in (evals_env / "labels.jsonl").read_text(encoding="utf-8") for canary in CANARIES)
