"""LLM-judged evals: `EVALS_LLM=1 evals/run.sh` answers, judges, and checks the judge against the owner.

For each `input:` + `expect:` case in evals/cases/ (synthetic, like every eval):
1. Answer: the answer route (config/models.yaml `judge.answer_route`) answers the case input,
   with the case's skill file as its system prompt.
2. Judge: the judge route (`judge.route`, the local one) grades the answer against each `expect`
   criterion, one criterion per call: PASS, FAIL, or UNKNOWN with one sentence of evidence
   (evals/rubric.md). A case passes when every criterion passes.
3. Agreement: the judge grades each answer the owner labeled in evals/labels.jsonl, and the report
   says how often it agrees with the owner. Judged failures fail the run only once the judge is
   trusted: >= 85% agreement on >= 20 labels (Zheng et al. 2023, "Judging LLM-as-a-Judge").

The owner labels with `evals/run.sh show [case]` (prints the case and its answer) and
`evals/run.sh label <case> pass|fail`. A label keeps the answer it judged, a hash of that answer,
and a hash of the case's criteria. An edited answer is an error, fixed by showing and labeling that
case again; a case whose criteria changed has a stale label, left out of the agreement until the
owner labels it again.

Model replies are cached by a hash of their inputs (URL, model, messages, limits) in `.metrics/evals/`,
so a rerun calls no model for an unchanged case. Replies are capped (ANSWER_TOKENS, JUDGE_TOKENS),
and Ollama routes are asked to turn thinking off; a server may ignore that request. HTTP responses
must report completion before they are graded or cached. Only repo text reaches a model: the skill file, the case input and criteria, and
the answer. Nothing sent comes from the data root.

Environment (this dev tool only, like `NO_CACHE` for scripts/check.sh):
- `EVALS_LLM=1`: `evals/run.sh` also runs this judge path;
- `EVALS_LABELS`: the labels file (default `evals/labels.jsonl`);
- `EVALS_CACHE`: the reply cache folder (default `.metrics/evals/`);
- `EVALS_LLM_URL`: an OpenAI-compatible base URL on this machine to use instead of the routes' own
  (another local server, or the e2e test's fake judge).
"""

from __future__ import annotations

import dataclasses
import errno
import hashlib
import json
import os
import re
import tempfile
import urllib.error
import urllib.request
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from . import calendar, models, paths, store

VERDICTS = ("PASS", "FAIL", "UNKNOWN")
LABELS = ("pass", "fail")
TARGET = 0.85  # the judge is trusted at or above this agreement with the owner...
MIN_LABELS = 20  # ...on at least this many labels
PIVOT = 0.70  # below this, switch to code graders per criterion (TASKS.md llm-evals)
TIMEOUT_S = 300  # a local model on a busy machine can take minutes per answer
ANSWER_TOKENS = 1024  # enough to say which commands to run and what to tell the user
JUDGE_TOKENS = 256  # one verdict and one sentence

ANSWER_SYSTEM = (
    "You are careerbrew, a private career assistant for programmers that works through the careerbrew CLI. "
    "You cannot run commands in this conversation: say which commands you would run, in order, and what you would tell the user. "
    "Follow the skill below when the request needs it, and only then.\n\n"
)
JUDGE_PROMPT = """You grade one answer of an AI career assistant against one criterion.
Reply with one JSON object and nothing else: {{"verdict": "PASS" or "FAIL" or "UNKNOWN", "evidence": "one sentence on the part of the answer that decides it"}}.
PASS: the answer meets the criterion. FAIL: it does not. UNKNOWN: the answer does not show either way.

Expected: the assistant should {should}.
User request: {input}
Criterion: {criterion}
Answer:
<<<
{answer}
>>>"""
_THINK = re.compile(r"<think>.*?</think>", re.S)


class LLMError(RuntimeError):
    """A route did not answer; the message names the fix."""


def _complete_http_transport(payload: dict[str, Any], url: str) -> str:
    """Accept only an HTTP completion that the server says finished."""
    headers = {"Content-Type": "application/json"}
    key_env = payload.pop("_api_key_env", "")
    if key_env and os.environ.get(key_env):
        headers["Authorization"] = f"Bearer {os.environ[key_env]}"
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers)
    with urllib.request.urlopen(req, timeout=TIMEOUT_S) as resp:  # noqa: S310 (configured URL)
        choice = json.loads(resp.read())["choices"][0]
    if not isinstance(choice, dict):
        raise LLMError("model completion has an invalid response shape")
    if choice.get("finish_reason") != "stop":
        raise LLMError("model completion is incomplete or has no completion reason; rerun with a complete response")
    return choice["message"]["content"]


@dataclass(frozen=True)
class Verdict:
    verdict: str  # PASS, FAIL, or UNKNOWN
    evidence: str


def digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def criteria(case: dict[str, Any]) -> list[str]:
    expect = case.get("expect") or []
    return [str(expect)] if isinstance(expect, str) else [str(x) for x in expect]


def expect_digest(case: dict[str, Any]) -> str:
    return digest("\n".join(criteria(case)))


def llm_cases(cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [c for c in cases if "cli" not in c]


def labels_path() -> Path:
    return Path(os.environ.get("EVALS_LABELS") or paths.REPO_ROOT / "evals" / "labels.jsonl")


def cache_dir() -> Path:
    return Path(os.environ.get("EVALS_CACHE") or paths.REPO_ROOT / ".metrics" / "evals")


def _checked_path(path: Path) -> Path:
    """Resolve eval paths without admitting protected career data."""
    try:
        resolved = path.resolve()
        if resolved.is_relative_to(paths.data_root(create=False).resolve()):
            raise LLMError("evaluation paths cannot use the data root (choose a folder outside it)")
        return resolved
    except (OSError, RuntimeError) as exc:
        if isinstance(exc, LLMError):
            raise
        raise LLMError("could not safely resolve an evaluation path") from None


def _read(path: Path) -> str:
    return _read_resolved(_checked_path(path))


def _read_resolved(resolved: Path) -> str:
    nofollow = getattr(os, "O_NOFOLLOW", None)
    if nofollow is None:
        raise LLMError("safe evaluation reads are unavailable on this platform")
    try:
        fd = os.open(resolved, os.O_RDONLY | nofollow)
    except OSError as exc:
        if exc.errno == errno.ELOOP:
            raise LLMError("evaluation path changed during the read; retry") from None
        raise
    with os.fdopen(fd, encoding="utf-8") as stream:
        return stream.read()


def source_path(path: Path, base: Path) -> Path:
    """Resolve a repo source inside its intended subtree and outside the data root."""
    repo = _checked_path(paths.REPO_ROOT)
    root = _checked_path(base)
    resolved = _checked_path(path)
    if not root.is_relative_to(repo) or not resolved.is_relative_to(root):
        raise LLMError("evaluation source must stay in its repository folder; use real source directories and in-folder files")
    return resolved


def repo_directory(*parts: str) -> Path:
    """Require the designated repository directory itself to stay in place."""
    repo = _checked_path(paths.REPO_ROOT)
    expected = repo.joinpath(*parts)
    if _checked_path(expected) != expected:
        raise LLMError("evaluation source must stay in its repository folder; use real source directories and in-folder files")
    return expected


def skill_text(name: str) -> str:
    if not name or name in {".", ".."} or any(c in name for c in ("/", "\\", "\0")):
        raise LLMError("evaluation skill must be a single skill name")
    folder = repo_directory("skills", name)
    skill = source_path(folder / "SKILL.md", folder)
    try:
        return _read_resolved(skill)
    except (FileNotFoundError, IsADirectoryError):
        return ""


def _write(path: Path, text: str) -> None:
    parent = _checked_path(path.parent)
    target = parent / path.name
    _checked_path(target)
    parent.mkdir(parents=True, exist_ok=True)
    if _checked_path(path.parent) != parent:
        raise LLMError("evaluation path changed during the write; retry")
    _checked_path(target)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=parent, prefix=".eval-", suffix=".tmp", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(text)
        _checked_path(target)
        os.replace(temporary, target)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def parse_verdict(text: str) -> Verdict:
    """Read the judge's reply: a JSON object first, then a bare PASS/FAIL/UNKNOWN; anything else is UNKNOWN."""
    body = _THINK.sub("", text).strip()
    m = re.search(r"\{.*\}", body, re.S)
    if m:
        try:
            obj = json.loads(m.group(0))
            v = str(obj.get("verdict", "")).strip().upper()
            if v in VERDICTS:
                return Verdict(v, " ".join(str(obj.get("evidence", "")).split())[:300])
        except (ValueError, AttributeError):
            pass
    word = re.search(r"\b(PASS|FAIL|UNKNOWN)\b", body)
    if word:
        return Verdict(word.group(1), " ".join(body.split())[:300])
    return Verdict("UNKNOWN", "the judge gave no verdict")


def case_verdict(verdicts: list[Verdict]) -> str:
    if verdicts and all(v.verdict == "PASS" for v in verdicts):
        return "pass"
    if any(v.verdict == "FAIL" for v in verdicts):
        return "fail"
    return "unknown"


def answer_messages(case: dict[str, Any]) -> list[dict[str, str]]:
    text = skill_text(str(case.get("skill", "")))
    return [{"role": "system", "content": ANSWER_SYSTEM + text}, {"role": "user", "content": str(case["input"])}]


def judge_messages(case: dict[str, Any], criterion: str, answer: str) -> list[dict[str, str]]:
    should = "hold back: not run its skill's steps for this request" if case.get("should") == "refrain" else "handle this request with its skill"
    return [{"role": "user", "content": JUDGE_PROMPT.format(should=should, input=case["input"], criterion=criterion, answer=answer)}]


def _route(name: str) -> models.Route:
    known = models.routes()
    route = next((r for r in known if r.name == name), None)
    if route is None:
        raise LLMError(f"config/models.yaml has no route {name!r} for the judge; use one of: {', '.join(r.name for r in known)}")
    url = os.environ.get("EVALS_LLM_URL", "").strip()
    if not url:
        return route
    route = dataclasses.replace(route, base_url=url)
    if not models._on_this_machine(route):
        raise LLMError(f"EVALS_LLM_URL must be on this machine (localhost, 127.0.0.1, or ::1), not {url}")
    return route


@dataclass
class Judge:
    answer_route: models.Route
    judge_route: models.Route
    transport: models.Transport
    cache: Path
    calls: int = 0  # model calls made (cache misses)

    @classmethod
    def from_config(cls, transport: models.Transport | None = None, cache: Path | None = None) -> Judge:
        cfg = (store.read_yaml(paths.config_dir() / "models.yaml", {}) or {}).get("judge") or {}
        judge_name = str(cfg.get("route", "local"))
        answer_name = str(cfg.get("answer_route", judge_name))
        return cls(_route(answer_name), _route(judge_name), transport or _complete_http_transport, cache or cache_dir())

    @staticmethod
    def _payload(route: models.Route, messages: list[dict[str, str]], max_tokens: int) -> dict[str, Any]:
        payload: dict[str, Any] = {"model": route.model, "messages": messages, "temperature": 0, "max_tokens": max_tokens}
        if route.provider == "ollama":
            payload["reasoning_effort"] = "none"  # a thinking model (qwen3) can think for minutes; the answer is what is graded
        return payload

    def _path(self, route: models.Route, payload: dict[str, Any]) -> Path:
        scope = "verified-http" if self.transport is _complete_http_transport else "injected-transport"
        return self.cache / f"{digest(json.dumps(['completion-v2', scope, route.base_url, payload], sort_keys=True))}.json"  # old entries lack completion evidence

    def _cached(self, route: models.Route, payload: dict[str, Any]) -> str | None:
        try:
            return str(json.loads(_read(self._path(route, payload)))["text"])
        except (OSError, ValueError, KeyError, TypeError):
            return None  # missing or broken: ask the model again

    def _call(self, route: models.Route, messages: list[dict[str, str]], max_tokens: int) -> str:
        payload = self._payload(route, messages, max_tokens)
        hit = self._cached(route, payload)
        if hit is not None:
            return hit
        _checked_path(self._path(route, payload))
        url = route.base_url.rstrip("/") + "/chat/completions"
        slow = f"route {route.name}: no reply from {route.base_url} within {TIMEOUT_S} s. Fix: run again when the machine is less busy (finished replies are cached), or use a smaller model for the {route.name} route"
        try:
            text = self.transport({**payload, "_api_key_env": route.api_key_env}, url)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                fix = f"`ollama pull {route.model}`, or name a model you have (`ollama list`)" if route.provider == "ollama" else "name a model the server has"
                raise LLMError(f"route {route.name}: model {route.model} is not available at {route.base_url} (HTTP 404). Fix: {fix} for the {route.name} route in config/models.yaml") from None
            raise LLMError(f"route {route.name}: HTTP {e.code} from {route.base_url}") from None
        except TimeoutError:
            raise LLMError(slow) from None
        except (urllib.error.URLError, OSError) as e:
            if isinstance(getattr(e, "reason", None), TimeoutError):
                raise LLMError(slow) from None
            raise LLMError(f"route {route.name}: nothing answers at {route.base_url} ({getattr(e, 'reason', e)}). Fix: start the local model server (`ollama serve`), or set EVALS_LLM_URL to a local one") from None
        except (KeyError, IndexError, TypeError, ValueError):
            raise LLMError(f"route {route.name}: the reply from {route.base_url} is not an OpenAI-style chat completion") from None
        if not isinstance(text, str):
            raise LLMError(f"route {route.name}: the reply from {route.base_url} has no text")
        self.calls += 1
        _write(self._path(route, payload), json.dumps({"text": text}))
        return text

    def cached_answer(self, case: dict[str, Any]) -> str | None:
        hit = self._cached(self.answer_route, self._payload(self.answer_route, answer_messages(case), ANSWER_TOKENS))
        return None if hit is None else _THINK.sub("", hit).strip()

    def answer(self, case: dict[str, Any]) -> str:
        return _THINK.sub("", self._call(self.answer_route, answer_messages(case), ANSWER_TOKENS)).strip()

    def grade(self, case: dict[str, Any], answer: str) -> list[Verdict]:
        """One judge call per criterion (cached), in criterion order."""
        return [parse_verdict(self._call(self.judge_route, judge_messages(case, c, answer), JUDGE_TOKENS)) for c in criteria(case)]


# Labels: evals/labels.jsonl, one JSON object per line:
# {"answer": "<the answer the owner read>", "answer_sha": "<digest(answer)>", "case": "<case id>",
#  "expect_sha": "<digest of the criteria>", "label": "pass" | "fail", "labeled": "YYYY-MM-DD"}
LABEL_KEYS = ("answer", "answer_sha", "case", "expect_sha", "label", "labeled")


def read_labels(path: Path | None = None, skip_case: str | None = None) -> tuple[list[dict[str, str]], list[str]]:
    """(valid labels, problems). A problem fails the run: the file is the owner's ground truth.

    `skip_case` leaves out that case's broken lines and their problems: `show` and `label` for that
    case run anyway, because labeling it again replaces those lines (the fix the problem names)."""
    path = path or labels_path()
    try:
        text = _read(path)
    except FileNotFoundError:
        return [], []
    out: list[dict[str, str]] = []
    problems: list[str] = []
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        where = f"{path.name} line {n}"
        try:
            row = json.loads(line)
        except ValueError:
            problems.append(f"{where}: not JSON")
            continue
        if not isinstance(row, dict) or not all(isinstance(row.get(k), str) for k in LABEL_KEYS):
            problems.append(f"{where}: needs the text fields {', '.join(LABEL_KEYS)}")
            continue
        if row["case"] == skip_case and (row["label"] not in LABELS or digest(row["answer"]) != row["answer_sha"]):
            continue
        if row["label"] not in LABELS:
            problems.append(f"{where} ({row['case']}): label must be pass or fail")
        elif digest(row["answer"]) != row["answer_sha"]:
            problems.append(f"{where} ({row['case']}): the answer changed after it was labeled; label it again: `evals/run.sh show {row['case']}`")
        else:
            out.append(row)
    return out, problems


def check_labels(labels: list[dict[str, str]], cases: list[dict[str, Any]]) -> list[str]:
    known = {c.get("id") for c in llm_cases(cases)}
    problems: list[str] = []
    seen: set[str] = set()
    for row in labels:
        if row["case"] not in known:
            problems.append(f"labels: {row['case']} is not an LLM-judged case in evals/cases/ (renamed or removed?)")
        if row["case"] in seen:
            problems.append(f"labels: {row['case']} is labeled twice; keep one line")
        seen.add(row["case"])
    return problems


def add_label(case: dict[str, Any], answer: str, label: str, path: Path | None = None) -> int:
    """Record (or replace) the owner's label for a case's answer. Returns the number of labels.

    It replaces the case's line, a broken one too; another broken line stops it, so no line is lost."""
    if label not in LABELS:
        raise ValueError("label must be pass or fail")
    path = path or labels_path()
    rows, problems = read_labels(path, skip_case=str(case["id"]))
    if problems:
        raise ValueError(f"fix {path} first: {'; '.join(problems)}")
    row = {"answer": answer, "answer_sha": digest(answer), "case": str(case["id"]), "expect_sha": expect_digest(case), "label": label, "labeled": calendar.now().date().isoformat()}
    at = next((i for i, r in enumerate(rows) if r["case"] == row["case"]), None)
    if at is None:
        rows.append(row)
    else:
        rows[at] = row
    _write(path, "".join(json.dumps(r, sort_keys=True, ensure_ascii=False) + "\n" for r in rows))
    return len(rows)


def current_labels(labels: list[dict[str, str]], cases: list[dict[str, Any]]) -> dict[str, dict[str, str]]:
    """Labels whose case still has the criteria the owner read, by case id."""
    by_id = {c.get("id"): c for c in llm_cases(cases)}
    return {r["case"]: r for r in labels if r["case"] in by_id and r["expect_sha"] == expect_digest(by_id[r["case"]])}


def next_unlabeled(labels: list[dict[str, str]], cases: list[dict[str, Any]]) -> dict[str, Any] | None:
    done = current_labels(labels, cases)
    return next((c for c in llm_cases(cases) if c.get("id") not in done), None)


@dataclass
class Agreement:
    rows: list[tuple[str, str, str]] = field(default_factory=list)  # (case, owner label, judge verdict)
    stale: list[str] = field(default_factory=list)  # labeled cases whose criteria changed since

    @property
    def n(self) -> int:
        return len(self.rows)

    @property
    def agree(self) -> int:
        return sum(1 for _, owner, judged in self.rows if owner == judged)

    @property
    def rate(self) -> float:
        return self.agree / self.n if self.n else 0.0

    @property
    def trusted(self) -> bool:
        return self.n >= MIN_LABELS and self.rate >= TARGET

    def decision(self) -> str:
        if self.n < MIN_LABELS:
            return f"COLLECT: {self.n}/{MIN_LABELS} labels; label more with `evals/run.sh show` and `evals/run.sh label <case> pass|fail`"
        if self.rate >= TARGET:
            return f"KEEP: the judge is trusted (>= {TARGET:.0%} on >= {MIN_LABELS} labels); judged regression failures now fail the run"
        if self.rate < PIVOT:
            return f"PIVOT: below {PIVOT:.0%}; switch to code graders per criterion (TASKS.md llm-evals)"
        return f"CALIBRATE: between {PIVOT:.0%} and {TARGET:.0%}; read the disagreements, fix the judge prompt or the criteria wording, and run again"


def agreement(judge: Judge, labels: list[dict[str, str]], cases: list[dict[str, Any]]) -> Agreement:
    """Grade every labeled answer with the judge and compare with the owner's label."""
    by_id = {c.get("id"): c for c in llm_cases(cases)}
    out = Agreement()
    for row in labels:
        case = by_id.get(row["case"])
        if case is None:
            continue
        if row["expect_sha"] != expect_digest(case):
            out.stale.append(row["case"])
            continue
        out.rows.append((row["case"], row["label"], case_verdict(judge.grade(case, row["answer"]))))
    return out


def agreement_lines(a: Agreement, judge: Judge) -> list[str]:
    route = f"route {judge.judge_route.name} ({judge.judge_route.model})"
    head = f"judge agreement: {a.agree}/{a.n} labels agree ({a.rate:.0%}) on {route}" if a.n else f"judge agreement: no owner labels yet ({route})"
    if a.stale:
        head += f"; {len(a.stale)} stale label(s) left out (criteria changed): {', '.join(a.stale)}"
    lines = [head]
    lines += [f"  disagree  {case}: owner {owner}, judge {judged}" for case, owner, judged in a.rows if owner != judged]
    lines.append(f"decision: {a.decision()}")
    return lines


def run(
    cases: list[dict[str, Any]],
    judge: Judge,
    labels: list[dict[str, str]],
    progress: Callable[[str], None] | None = None,
) -> tuple[list[str], list[str]]:
    """Answer and judge every LLM case, then check the judge against the labels: (report lines, failures).

    Judged failures of regression cases are failures only when the judge is trusted."""
    judged: list[tuple[dict[str, Any], list[Verdict]]] = []
    todo = llm_cases(cases)
    for i, case in enumerate(todo, 1):
        before = judge.calls
        verdicts = judge.grade(case, judge.answer(case))
        judged.append((case, verdicts))
        if progress and judge.calls > before:
            progress(f"llm: {i}/{len(todo)} {case.get('id')}")
    a = agreement(judge, labels, cases)
    counts = Counter(case_verdict(v) for _, v in judged)
    ar, jr = judge.answer_route, judge.judge_route
    lines = [f"llm: {len(judged)} case(s) answered on route {ar.name} ({ar.model}) and judged on route {jr.name} ({jr.model}): {counts['pass']} pass, {counts['fail']} fail, {counts['unknown']} unknown"]
    failures: list[str] = []
    for case, verdicts in judged:
        result = case_verdict(verdicts)
        if result == "pass":
            continue
        first = next((i, v) for i, v in enumerate(verdicts, 1) if v.verdict != "PASS") if verdicts else (0, Verdict("UNKNOWN", "no criteria"))
        msg = f"{case.get('id')}: judged {result} (criterion {first[0]}: {first[1].verdict}, {first[1].evidence})"
        if a.trusted and case.get("suite", "regression") == "regression":
            failures.append(msg)
        else:
            lines.append(f"  {msg}" + ("" if a.trusted else " [judge not trusted yet]"))
    return lines + agreement_lines(a, judge), failures


def show_text(case: dict[str, Any], answer: str, judge: Judge, labels: list[dict[str, str]], cases: list[dict[str, Any]]) -> str:
    done = current_labels(labels, cases)
    lines = [
        f"case: {case['id']} (skill {case.get('skill')}, should {case.get('should')}, suite {case.get('suite', 'regression')})",
        f"input: {case['input']}",
        "criteria (pass means the answer meets every one):",
        *[f"  {i}. {c}" for i, c in enumerate(criteria(case), 1)],
        f"answer (route {judge.answer_route.name}, {judge.answer_route.model}):",
        answer,
        "---",
    ]
    mine = done.get(case["id"])
    if mine and mine["answer_sha"] == digest(answer):
        lines.append(f"your label: {mine['label']} ({mine['labeled']})")
    lines.append(f"label it: evals/run.sh label {case['id']} pass   (or fail)")
    lines.append(f"labels: {len(done)}/{MIN_LABELS}")
    return "\n".join(lines)
