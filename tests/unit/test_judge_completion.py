"""The evaluator refuses incomplete HTTP responses and old unqualified cache entries."""

import json

import pytest

from careerbrew import judge, paths


@pytest.fixture
def setup_judge(tmp_path, monkeypatch):
    config = tmp_path / "config"
    config.mkdir()
    (config / "models.yaml").write_text("routes:\n  - name: local\n    provider: ollama\n    base_url: http://localhost:11434/v1\n    model: fixture:1b\n    private_data: true\njudge:\n  route: local\n  answer_route: local\n")
    monkeypatch.setattr(paths, "config_dir", lambda: config)
    monkeypatch.setattr(judge, "skill_text", lambda name: "synthetic instructions")
    responses = []
    calls = []

    class Response:
        def __init__(self, value):
            self.value = value

        def __enter__(self):
            return self

        def __exit__(self, *args):
            return None

        def read(self):
            return json.dumps(self.value).encode()

    def urlopen(req, timeout):
        calls.append(req)
        return Response(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", urlopen)
    j = judge.Judge.from_config(cache=tmp_path / "cache")
    case = {"id": "fixture", "skill": "synthetic", "input": "synthetic request", "expect": ["one result"]}
    return j, case, responses, calls


def reply(content, reason):
    choice = {"message": {"content": content}}
    if reason is not None:
        choice["finish_reason"] = reason
    return {"choices": [choice]}


def test_complete_answer_is_accepted_and_reused(setup_judge):
    j, case, responses, calls = setup_judge
    responses.append(reply("complete answer", "stop"))
    assert j.answer(case) == "complete answer"
    assert j.answer(case) == "complete answer"
    assert len(calls) == 1


def test_incomplete_or_unqualified_answer_is_not_cached(setup_judge):
    j, case, responses, calls = setup_judge
    for reason in ("length", None):
        responses.append(reply("partial answer", reason))
        with pytest.raises(judge.LLMError, match="incomplete|completion"):
            j.answer(case)
    assert len(calls) == 2
    assert not list(j.cache.glob("*.json"))


def test_incomplete_or_unqualified_verdict_is_not_accepted(setup_judge):
    j, case, responses, calls = setup_judge
    for reason in ("length", None):
        responses.append(reply('{"verdict":"PASS","evidence":"partial"}', reason))
        with pytest.raises(judge.LLMError, match="incomplete|completion"):
            j.grade(case, "answer")
    for invalid in (None, "invalid"):
        responses.append({"choices": [invalid]})
        with pytest.raises(judge.LLMError):
            j.grade(case, "answer")
    assert len(calls) == 4
    assert not list(j.cache.glob("*.json"))


def test_legacy_cache_without_completion_evidence_is_not_reused(setup_judge):
    j, case, responses, calls = setup_judge
    payload = j._payload(j.answer_route, judge.answer_messages(case), judge.ANSWER_TOKENS)
    old_key = judge.digest(json.dumps([j.answer_route.base_url, payload], sort_keys=True))
    j.cache.mkdir(parents=True)
    old = j.cache / f"{old_key}.json"
    old.write_text(json.dumps({"text": "legacy partial"}))
    responses.append(reply("fresh complete answer", "stop"))
    assert j.answer(case) == "fresh complete answer"
    assert len(calls) == 1
    assert old.exists()
    other_case = dict(case, input="another synthetic request")
    injected = judge.Judge.from_config(transport=lambda payload, url: "unqualified partial", cache=j.cache)
    assert injected.answer(other_case) == "unqualified partial"
    responses.append(reply("verified fresh answer", "stop"))
    assert j.answer(other_case) == "verified fresh answer"
    assert len(calls) == 2
