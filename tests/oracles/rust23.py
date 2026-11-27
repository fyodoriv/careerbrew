"""rust-23 Python oracle for model routes, the eval judge adapter and local voice.

Every case uses invocation-owned synthetic data, fixed clocks, fake speech
engines and a local HTTP fixture server on a free OS port. No real model,
network host, owner file or speech engine is used. Reports hold counts, ids,
digests and synthetic text only; audio and transcripts stay temporary.
"""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import os
import re
import socket
import sys
import threading
import time
import urllib.error
from argparse import Namespace
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from careerbrew import cli, judge, models, paths, store
from careerbrew.tutor import voice

CASES = (
    "routes-config",
    "allowed-routes",
    "complete-routing",
    "judge-text",
    "judge-cache",
    "http-transports",
    "labels",
    "agreement-run",
    "voice-transcribe",
    "voice-answer",
)
ROOT = Path(__file__).resolve().parents[2]
VOICE = ROOT / "tests" / "fixtures" / "voice"
CANARY = "RUST23CANARYTERM"
TOKEN = "rust23-synthetic-token"
REAL_CONFIG_DIR = paths.config_dir

# The oracle points route reads at a synthetic config folder for most cases.
# This replacement has process scope only in the oracle invocation.
CONFIG_DIR: list[Path | None] = [None]
paths.config_dir = lambda: CONFIG_DIR[0] or REAL_CONFIG_DIR()  # type: ignore[assignment]


def _clean(text: str) -> str:
    """Replace invocation-owned paths and temp names with fixed placeholders."""
    for raw, mark in ((OWNED[0], "<data>"), (TEMP[0], "<tmp>")):
        if raw:
            text = text.replace(raw, mark)
    return TEMP_NAME.sub("careerbrew-voice-<id>", text)


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool):
        return value
    if isinstance(value, str):
        return _clean(value)
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, dict):
        return {_clean(str(key)): _typed(value[key]) for key in sorted(value, key=str)}
    return {"type": type(value).__name__}


def _pure(name: str) -> str:
    raw = os.environ.get(name, "")
    return str(Path(raw)) if raw else ""


OWNED = [_pure("CAREERBREW_HOME")]
TEMP = [_pure("TMPDIR")]
TEMP_NAME = re.compile(r"careerbrew-voice-[A-Za-z0-9_]+")


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _outcome(fn: Any) -> Any:
    try:
        return {"ok": fn()}
    except Exception as exc:
        return {"error": type(exc).__name__, "message": str(exc)}


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.bin = self.base / "bin"
        self.config = self.base / "config"
        self.root.mkdir(parents=True)
        self.root.chmod(0o700)
        self.bin.mkdir(parents=True)
        self.config.mkdir(parents=True)
        for key in ("CLOUDFLARE_ACCOUNT_ID", "EVALS_LLM_URL", "EVALS_LABELS", "EVALS_CACHE", "RUST23_TOKEN"):
            os.environ.pop(key, None)
        os.environ.update(
            {
                "HOME": str(self.base / "home"),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "on",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_LLM": "on",
                "CAREERBREW_NOW": "2026-10-19T13:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "PATH": str(self.bin),
                "NO_PROXY": "*",
                "no_proxy": "*",
                "TZ": "UTC",
            }
        )
        time.tzset()
        CONFIG_DIR[0] = self.config

    def models_yaml(self, text: str) -> None:
        (self.config / "models.yaml").write_text(text, encoding="utf-8")

    def script(self, name: str, body: str) -> Path:
        path = self.bin / name
        path.write_text("#!/bin/sh\n" + body, encoding="utf-8")
        path.chmod(0o755)
        return path


def _events(ws: Workspace) -> list[dict[str, Any]]:
    path = ws.root / "logs" / "events.jsonl"
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        row = json.loads(line)
        row.pop("ts", None)
        rows.append(row)
    return rows


def _route(r: models.Route) -> dict[str, Any]:
    return {
        "name": r.name,
        "provider": r.provider,
        "model": r.model,
        "private_data": r.private_data,
        "base_url": r.base_url,
        "api_key_env": r.api_key_env,
        "on_this_machine": models._on_this_machine(r),
    }


# --- local HTTP fixture server -------------------------------------------------


class Fixture:
    """Scripted chat-completion server on 127.0.0.1 and a free OS port."""

    def __init__(self, replies: list[tuple[int, bytes]]):
        self.replies = list(replies)
        self.seen: list[dict[str, Any]] = []
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802 (http.server API)
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length)
                try:
                    parsed = json.loads(body)
                except ValueError:
                    parsed = None
                fixture.seen.append(
                    {
                        "path": self.path,
                        "auth": self.headers.get("Authorization") == f"Bearer {TOKEN}",
                        "auth_present": self.headers.get("Authorization") is not None,
                        "content_type": self.headers.get("Content-Type"),
                        "body": parsed,
                    }
                )
                status, reply = fixture.replies.pop(0) if fixture.replies else (500, b"{}")
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(reply)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(reply)

            def log_message(self, *_args: Any) -> None:
                return None

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def url(self) -> str:
        return f"http://127.0.0.1:{self.port}/v1"

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


def _closed_port() -> int:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


def _completion(content: Any, finish: Any = "stop", **extra: Any) -> bytes:
    choice: dict[str, Any] = {"message": {"content": content}}
    if finish is not None:
        choice["finish_reason"] = finish
    choice.update(extra)
    return json.dumps({"choices": [choice]}).encode()


# --- cases -----------------------------------------------------------------------

SYNTH_ROUTES = """routes:
  - name: local
    provider: ollama
    base_url: http://localhost:11434/v1
    model: synth-local:1b
    private_data: true
  - name: lan
    provider: ollama
    base_url: http://10.0.0.5:11434/v1
    model: synth-lan
    private_data: true
  - name: loop6
    provider: openai-compatible
    base_url: http://[::1]:8080/v1
    model: 8
    private_data: yes
  - name: hosted
    provider: synthetic-hosted
    base_url: https://${RUST23_ACCOUNT}.example.invalid/$RUST23_MISSING/v1
    model: synth-hosted
    api_key_env: RUST23_TOKEN
    private_data: false
judge:
  route: local
  answer_route: local
"""


def _routes_config(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "routes-config")
    CONFIG_DIR[0] = None
    real_unset = [_route(r) for r in models.routes()]
    os.environ["CLOUDFLARE_ACCOUNT_ID"] = "synthacct"
    real_set = [_route(r) for r in models.routes()]
    os.environ.pop("CLOUDFLARE_ACCOUNT_ID")
    CONFIG_DIR[0] = ws.config
    ws.models_yaml(SYNTH_ROUTES)
    os.environ["RUST23_ACCOUNT"] = "acct-7"
    synth = [_route(r) for r in models.routes()]
    os.environ.pop("RUST23_ACCOUNT")
    ws.models_yaml("judge: {route: local}\n")
    no_routes = [_route(r) for r in models.routes()]
    missing = ws.config / "models.yaml"
    missing.unlink()
    missing_file = [_route(r) for r in models.routes()]
    ws.models_yaml("routes:\n  - provider: ollama\n")
    nameless = _outcome(lambda: [_route(r) for r in models.routes()])
    hosts = {}
    for url in ("http://LOCALHOST:1/v1", "http://127.0.0.1/v1", "http://[::1]/v1", "http://user@localhost:9/", "localhost:11434/v1", "", "http://localhost.example.invalid/v1", "https://127.0.0.2/v1"):
        hosts[url] = models._on_this_machine(models.Route("h", "p", "m", True, base_url=url))
    return {"real_unset": real_unset, "real_set": real_set, "synthetic": synth, "no_routes": no_routes, "missing_file": missing_file, "nameless": nameless, "hosts": hosts}


def _allowed_names(sensitivity: str) -> Any:
    return _outcome(lambda: [r.name for r in models.allowed_routes(sensitivity)])


def _allowed_routes(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "allowed-routes")
    ws.models_yaml(SYNTH_ROUTES)
    sens = ("public", "personal", "business", "bogus")
    personal = {s: _allowed_names(s) for s in sens}
    plugin_root = ws.root / "plugins" / "rust23plug"
    store.write_yaml(plugin_root / "careerbrew-plugin.yaml", {"name": "rust23plug", "version": "0.1.0", "policy": {"allowed_routes": ["local", "hosted", "lan"]}})
    store.write_json(ws.root / "plugins.lock.json", {"plugins": {"rust23plug": {"origin": "plugin:rust23plug"}}})
    narrowed = {s: _allowed_names(s) for s in sens}
    store.write_yaml(ws.root / "machine.yaml", {"role": "corporate", "label": "synthetic"})
    corporate = {s: _allowed_names(s) for s in sens}
    return {"personal": personal, "plugin": narrowed, "corporate": corporate}


class FakeTransport:
    def __init__(self, script: list[Any]):
        self.script = list(script)
        self.seen: list[dict[str, Any]] = []

    def __call__(self, payload: dict[str, Any], url: str) -> Any:
        seen = dict(payload)
        seen["messages"] = hashlib.sha256(json.dumps(payload.get("messages"), sort_keys=True).encode()).hexdigest()[:16]
        self.seen.append({"payload": seen, "url": url})
        step = self.script.pop(0)
        if isinstance(step, BaseException):
            raise step
        return step


def _complete_routing(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "complete-routing")
    ws.models_yaml(SYNTH_ROUTES)
    store.write_text(ws.root / "sensitive-terms.txt", CANARY + "\n")
    out: dict[str, Any] = {}

    def run(label: str, prompt: str, sensitivity: str, script: list[Any]) -> None:
        fake = FakeTransport(script)
        out[label] = {"result": _outcome(lambda: list(models.complete(prompt, sensitivity, lambda: f"fallback for {label}", transport=fake))), "seen": fake.seen}

    os.environ["CAREERBREW_LLM"] = "off"
    run("llm-off", "synthetic prompt", "public", ["never"])
    os.environ["CAREERBREW_LLM"] = "on"
    run("first-answers", "synthetic prompt", "public", ["local answer"])
    run("unavailable-then-next", "synthetic prompt", "public", [urllib.error.URLError("refused"), OSError("down"), ValueError("bad"), "hosted answer"])
    run("all-unavailable", "synthetic prompt", "public", [KeyError("choices"), TimeoutError("slow"), urllib.error.HTTPError("u", 503, "busy", None, None), OSError("none")])  # type: ignore[arg-type]
    run("privacy-block", f"about {CANARY} plans", "public", [OSError("down"), OSError("down"), OSError("down"), "never"])
    run("personal-private-only", f"about {CANARY} plans", "personal", [urllib.error.URLError("no"), "lan answer", "unused"])
    run("bad-sensitivity", "x", "secret", [])
    run("uncaught-runtime", "synthetic prompt", "public", [RuntimeError("boom")])
    out["events"] = _events(ws)
    return out


def _judge_text(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "judge-text")
    ws.models_yaml(SYNTH_ROUTES)
    replies = [
        '{"verdict": "PASS", "evidence": "names the   command"}',
        '<think>{"verdict": "FAIL"}</think>{"verdict": "fail", "evidence": "missing step"}',
        'Sure: {"verdict": "UNKNOWN"} trailing',
        '{"verdict": "MAYBE"} but PASS overall',
        '{"verdict": PASS}',
        "no verdict here",
        "the answer FAILs; verdict: FAIL.",
        '{"verdict": " pass ", "evidence": null}',
        '{"verdict": "PASS", "evidence": 12}',
        '{"verdict": "PASS", "evidence": true}',
        '{"verdict": "PASS", "evidence": "' + "long " * 80 + '"}',
        "[1, 2] PASS",
        '{"evidence": "only evidence"} UNKNOWN',
        "<think>PASS</think>",
        "PASSING grade",
        "verdict: Pass",
        '{"verdict": "PASS", "evidence": "a\\u00e9b"}',
        "x" * 10 + " UNKNOWN " + "y\n" * 200,
    ]
    parsed = [[judge.parse_verdict(r).verdict, judge.parse_verdict(r).evidence] for r in replies]
    V = judge.Verdict
    case_verdicts = [
        judge.case_verdict([]),
        judge.case_verdict([V("PASS", ""), V("PASS", "")]),
        judge.case_verdict([V("PASS", ""), V("UNKNOWN", "")]),
        judge.case_verdict([V("UNKNOWN", ""), V("FAIL", "")]),
    ]
    cases = [
        {"id": "c1", "skill": "rust23-none", "input": "plan my week", "expect": ["names a command", "asks one question"], "should": "use"},
        {"id": "c2", "skill": "rust23-none", "input": "tell a joke", "expect": "refuses politely", "should": "refrain"},
        {"id": "c3", "cli": ["status"], "expect": "x"},
        {"id": "c4", "skill": "rust23-none", "input": "number criteria", "expect": [1, 2.5, True]},
        {"id": "c5", "skill": "rust23-none", "input": "no criteria"},
    ]
    criteria = {c["id"]: judge.criteria(c) for c in cases}
    digests = {c["id"]: judge.expect_digest(c) for c in cases}
    llm_ids = [c["id"] for c in judge.llm_cases(cases)]
    judge_msgs = [judge.judge_messages(cases[0], "names a command", "run careerbrew plan"), judge.judge_messages(cases[1], "refuses politely", "no")]
    answer_msgs = judge.answer_messages(cases[0])
    real_skill = judge.skill_text("careerbrew-teacher")
    skill_names = {}
    for name in ("", ".", "..", "a/b", "a\\b", "a\0b", "rust23-none"):
        skill_names[repr(name)] = _outcome(lambda name=name: judge.skill_text(name))
    real_answer = judge.answer_messages({"skill": "careerbrew-teacher", "input": "teach me"})
    routes = {}
    for name in ("local", "lan", "nope"):
        routes[name] = _outcome(lambda name=name: _route(judge._route(name)))
    os.environ["EVALS_LLM_URL"] = " http://127.0.0.1:9/v1 "
    routes["override-local"] = _outcome(lambda: _route(judge._route("hosted")))
    os.environ["EVALS_LLM_URL"] = "https://judge.example.invalid/v1"
    routes["override-remote"] = _outcome(lambda: _route(judge._route("local")))
    os.environ["EVALS_LLM_URL"] = "   "
    routes["override-blank"] = _outcome(lambda: _route(judge._route("lan")))
    os.environ.pop("EVALS_LLM_URL")
    local = judge._route("local")
    lan = judge._route("loop6")
    payloads = [judge.Judge._payload(local, answer_msgs, judge.ANSWER_TOKENS), judge.Judge._payload(lan, judge_msgs[0], judge.JUDGE_TOKENS)]
    A = judge.Agreement
    rows20 = [(f"k{i}", "pass", "pass") for i in range(20)]
    agreements = {
        "empty": A(),
        "few": A(rows=[("a", "pass", "pass"), ("b", "fail", "pass")], stale=["s1"]),
        "keep": A(rows=rows20[:17] + [("x1", "pass", "fail"), ("x2", "fail", "pass"), ("x3", "fail", "unknown")]),
        "pivot": A(rows=rows20[:13] + [(f"y{i}", "pass", "fail") for i in range(7)]),
        "calibrate": A(rows=rows20[:15] + [(f"z{i}", "pass", "fail") for i in range(5)], stale=["s1", "s2"]),
        "two-thirds": A(rows=rows20[:14] + [(f"w{i}", "pass", "fail") for i in range(7)]),
    }
    jdg = judge.Judge(local, lan, lambda p, u: "", ws.base / "cache")
    agreement_report = {}
    for key, a in agreements.items():
        agreement_report[key] = {"n": a.n, "agree": a.agree, "trusted": a.trusted, "decision": a.decision(), "lines": judge.agreement_lines(a, jdg)}
    return {
        "parsed": parsed,
        "case_verdicts": case_verdicts,
        "digest": [judge.digest(""), judge.digest("héllo"), judge.digest("abc")],
        "criteria": criteria,
        "expect_digests": digests,
        "llm_ids": llm_ids,
        "judge_messages": judge_msgs,
        "answer_messages": answer_msgs,
        "real_skill_sha": hashlib.sha256(real_skill.encode()).hexdigest(),
        "real_answer_sha": hashlib.sha256(json.dumps(real_answer, sort_keys=True).encode()).hexdigest(),
        "skill_names": skill_names,
        "routes": routes,
        "payloads": payloads,
        "agreements": agreement_report,
    }


def _cache_listing(folder: Path) -> list[list[Any]]:
    if not folder.exists():
        return []
    return [[p.name, oct(p.stat().st_mode & 0o777), p.read_text(encoding="utf-8")] for p in sorted(folder.iterdir())]


def _judge_cache(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "judge-cache")
    ws.models_yaml(SYNTH_ROUTES.replace("  answer_route: local\n", "  answer_route: loop6\n"))

    def scripted(payload: dict[str, Any], url: str) -> str:
        content = payload["messages"][-1]["content"]
        if content.startswith("You grade"):
            return '{"verdict": "PASS", "evidence": "synthetic"}' if "names" in content else '{"verdict": "FAIL", "evidence": "synthetic"}'
        return "<think>plan</think>  Run `careerbrew plan` and name it.  "

    cache = ws.base / "cache"
    j = judge.Judge.from_config(transport=scripted, cache=cache)
    case = {"id": "c1", "skill": "rust23-none", "input": "plan my week", "expect": ["names a command", "asks one question"]}
    before = j.cached_answer(case)
    ans = j.answer(case)
    grades = [[v.verdict, v.evidence] for v in j.grade(case, ans)]
    calls_first = j.calls
    ans2 = j.answer(case)
    grades2 = [[v.verdict, v.evidence] for v in j.grade(case, ans2)]
    calls_second = j.calls
    after = j.cached_answer(case)
    listing = _cache_listing(cache)
    first = sorted(cache.iterdir())[0]
    first.write_text("{broken", encoding="utf-8")
    j.grade(case, ans)
    j.answer(case)
    calls_repaired = j.calls

    errors: dict[str, Any] = {}
    error_cases = [
        ("http-404-ollama", "local", urllib.error.HTTPError("u", 404, "nf", None, None)),  # type: ignore[arg-type]
        ("http-404-other", "loop6", urllib.error.HTTPError("u", 404, "nf", None, None)),  # type: ignore[arg-type]
        ("http-500", "local", urllib.error.HTTPError("u", 500, "err", None, None)),  # type: ignore[arg-type]
        ("timeout", "local", TimeoutError("slow")),
        ("url-timeout", "local", urllib.error.URLError(TimeoutError("slow"))),
        ("url-refused", "local", urllib.error.URLError("refused")),
        ("os-error", "local", OSError("no route")),
        ("key-error", "local", KeyError("choices")),
        ("index-error", "local", IndexError("0")),
        ("type-error", "local", TypeError("x")),
        ("value-error", "local", ValueError("x")),
        ("non-text", "local", 5),
        ("llm-error", "local", judge.LLMError("model completion is incomplete or has no completion reason; rerun with a complete response")),
    ]
    for n, (label, route, step) in enumerate(error_cases):
        fake = FakeTransport([step])
        jj = judge.Judge(judge._route(route), judge._route(route), fake, ws.base / "error-cache")
        res = _outcome(lambda jj=jj, n=n: jj.answer({"skill": "rust23-none", "input": f"error {n}"}))
        errors[label] = {"result": res, "calls": jj.calls, "seen_keys": sorted(fake.seen[0]["payload"]) if fake.seen else [], "url": fake.seen[0]["url"] if fake.seen else None}
    inside = judge.Judge(judge._route("local"), judge._route("local"), FakeTransport(["x"]), ws.root / "cache")
    inside_result = _outcome(lambda: inside.answer({"skill": "rust23-none", "input": "inside"}))
    return {
        "routes": [j.answer_route.name, j.judge_route.name],
        "cached_before": before,
        "answer": ans,
        "grades": grades,
        "calls_first": calls_first,
        "answer_again": ans2,
        "grades_again": grades2,
        "calls_second": calls_second,
        "cached_after": after,
        "listing": listing,
        "calls_repaired": calls_repaired,
        "errors": errors,
        "error_cache": len(list((ws.base / "error-cache").iterdir())) if (ws.base / "error-cache").exists() else 0,
        "inside_data_root": inside_result,
    }


def _http_transports(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "http-transports")
    replies = [
        (200, _completion("HTTP answer one")),
        (200, _completion("cut off", finish="length")),
        (200, _completion("no reason", finish=None)),
        (200, json.dumps({"choices": ["x"]}).encode()),
        (200, b"not json"),
        (200, b"{}"),
        (404, b'{"error": "missing"}'),
        (500, b"{}"),
        (200, _completion(5)),
        (200, json.dumps({"choices": [{"finish_reason": "stop"}]}).encode()),
        (200, json.dumps({"choices": [{"message": {"content": "complete via http"}}]}).encode()),
        (503, b"{}"),
    ]
    fixture = Fixture(replies)
    try:
        ws.models_yaml(SYNTH_ROUTES.replace("    model: synth-local:1b\n", "    model: synth-local:1b\n    api_key_env: RUST23_TOKEN\n"))
        os.environ["RUST23_TOKEN"] = TOKEN
        os.environ["EVALS_LLM_URL"] = fixture.url()
        j = judge.Judge.from_config(cache=ws.base / "cache")
        results = []
        for n in range(10):
            results.append(_outcome(lambda n=n: j.answer({"skill": "rust23-none", "input": f"http {n}"})))
        os.environ.pop("EVALS_LLM_URL")
        cached = len(list((ws.base / "cache").iterdir()))
        ws.models_yaml(f"routes:\n  - name: fixture\n    provider: ollama\n    base_url: {fixture.url()}/\n    model: synth\n    private_data: true\n")
        complete_ok = _outcome(lambda: list(models.complete("synthetic prompt", "personal", lambda: "fallback")))
        complete_down = _outcome(lambda: list(models.complete("synthetic prompt", "personal", lambda: "fallback")))
        seen = fixture.seen
    finally:
        fixture.close()
    closed = _closed_port()
    os.environ["EVALS_LLM_URL"] = f"http://127.0.0.1:{closed}/v1"
    ws.models_yaml(SYNTH_ROUTES)
    refused = _outcome(lambda: judge.Judge.from_config(cache=ws.base / "cache2").answer({"skill": "rust23-none", "input": "refused"}))
    if "error" in refused:
        refused["message"] = refused["message"].replace(str(closed), "<port>").split(" (", 1)[0]
    os.environ.pop("EVALS_LLM_URL")
    for row in results:
        if "error" in row:
            row["message"] = row["message"].replace(str(fixture.port), "<port>")
    return {"results": results, "cached": cached, "complete_ok": complete_ok, "complete_down": complete_down, "seen": seen, "refused": refused, "events": _events(ws)}


def _labels(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "labels")
    labels = ws.base / "labels" / "labels.jsonl"
    os.environ["EVALS_LABELS"] = str(labels)
    cases = [
        {"id": "c1", "skill": "s", "input": "one", "expect": ["a", "b"]},
        {"id": "c2", "skill": "s", "input": "two", "expect": "c"},
        {"id": "c3", "cli": ["x"], "expect": "d"},
        {"id": "c4", "skill": "s", "input": "four", "expect": ["e"]},
    ]
    missing = judge.read_labels()
    added = [judge.add_label(cases[0], "answer one é", "pass"), judge.add_label(cases[1], "answer two", "fail")]
    replaced = judge.add_label(cases[0], "answer one v2", "fail")
    bad_label = _outcome(lambda: judge.add_label(cases[1], "x", "maybe"))
    text_after_add = labels.read_text(encoding="utf-8")
    mode = oct(labels.stat().st_mode & 0o777)
    good = json.loads(text_after_add.splitlines()[0])
    lines = [
        text_after_add.splitlines()[0],
        "",
        "not json",
        "[1, 2]",
        json.dumps({"case": "c2"}),
        json.dumps({**good, "case": "c4", "label": "maybe"}),
        json.dumps({**good, "case": "c4", "answer": "edited"}),
        json.dumps({**good, "case": "c9"}),
        text_after_add.splitlines()[0],
        json.dumps({**good, "case": "c3"}),
    ]
    labels.write_text("\n".join(lines) + "\n", encoding="utf-8")
    read_all = judge.read_labels()
    read_skip = judge.read_labels(skip_case="c4")
    checked = judge.check_labels(read_all[0], cases)
    blocked = _outcome(lambda: judge.add_label(cases[0], "x", "pass"))
    labels.write_text("\n".join([lines[0], lines[5], lines[6]]) + "\n", encoding="utf-8")
    fixed = judge.add_label(cases[3], "answer four", "pass")
    final_text = labels.read_text(encoding="utf-8")
    final = judge.read_labels()[0]
    current = sorted(judge.current_labels(final, cases))
    changed_cases = [dict(cases[0], expect=["a", "changed"]), cases[1], cases[3]]
    stale_current = sorted(judge.current_labels(final, changed_cases))
    nxt = judge.next_unlabeled(final, changed_cases)
    none_left = judge.next_unlabeled(final, [cases[0], cases[3]])
    os.environ["EVALS_LABELS"] = str(ws.root / "labels.jsonl")
    inside = _outcome(lambda: judge.read_labels())
    inside_add = _outcome(lambda: judge.add_label(cases[0], "x", "pass"))
    os.environ.pop("EVALS_LABELS")
    return {
        "missing": missing,
        "added": added,
        "replaced": replaced,
        "bad_label": bad_label,
        "text_after_add": text_after_add,
        "mode": mode,
        "read_all": read_all,
        "read_skip": read_skip,
        "checked": checked,
        "blocked": blocked,
        "fixed": fixed,
        "final_text": final_text,
        "current": current,
        "stale_current": stale_current,
        "next": nxt["id"] if nxt else None,
        "none_left": none_left,
        "inside": inside,
        "inside_add": inside_add,
    }


def _agreement_run(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "agreement-run")
    ws.models_yaml(SYNTH_ROUTES)
    cases = [{"id": f"r{i:02d}", "skill": "rust23-none", "input": f"request {i}", "expect": ["names a command", "keeps it short" if i % 3 else "fails on purpose"], "suite": "regression" if i % 4 else "capability"} for i in range(22)]
    cases.append({"id": "cli-case", "cli": ["status"], "expect": "x"})

    def scripted(payload: dict[str, Any], url: str) -> str:
        content = payload["messages"][-1]["content"]
        if content.startswith("You grade"):
            if "fails on purpose" in content:
                return '{"verdict": "FAIL", "evidence": "synthetic miss"}'
            if "request 5" in content:
                return "UNKNOWN"
            return '{"verdict": "PASS", "evidence": "synthetic hit"}'
        return f"answer for {content}"

    labels_file = ws.base / "labels.jsonl"
    os.environ["EVALS_LABELS"] = str(labels_file)
    j = judge.Judge(judge._route("local"), judge._route("lan"), scripted, ws.base / "cache")
    progress: list[str] = []
    few = [judge.add_label(cases[i], j.answer(cases[i]), "pass" if i % 3 else "fail") for i in range(3)]
    labels = judge.read_labels()[0]
    untrusted = judge.run(cases, j, labels, progress.append)
    for i in range(3, 22):
        judge.add_label(cases[i], j.answer(cases[i]), "pass" if (i % 3 and i != 5) else "fail")
    labels = judge.read_labels()[0]
    stale_cases = [dict(cases[1], expect=["changed"])] + cases[2:]
    trusted = judge.run(stale_cases, j, labels, progress.append)
    show = judge.show_text(cases[1], j.answer(cases[1]), j, labels, cases)
    show_new = judge.show_text(cases[1], "different answer", j, labels, stale_cases)
    os.environ.pop("EVALS_LABELS")
    return {"few": few, "untrusted": untrusted, "trusted": trusted, "progress": progress, "calls": j.calls, "show": show, "show_new": show_new}


def _settings(ws: Workspace, values: dict[str, Any]) -> None:
    store.write_yaml(ws.root / "settings.yaml", values)


def _voice_transcribe(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "voice-transcribe")
    log = ws.base / "engine.log"
    audio = ws.base / "answer.wav"
    audio.write_bytes((VOICE / "behavioral-answer.wav").read_bytes())
    ogg = ws.base / "answer.ogg"
    ogg.write_bytes(b"OggS synthetic")
    out: dict[str, Any] = {}
    out["no-engine-cmd"] = voice.stt_command()
    out["no-engine"] = _outcome(lambda: voice.transcribe(audio))
    _settings(ws, {"stt_model": "~/models/ggml base.bin"})
    out["model-without-whisper"] = voice.stt_command()
    ws.script("whisper-cli", f'/bin/echo "$@" > "{log}"\n/bin/echo "[00:00.000 --> 00:01.000]  whisper says hi"\n')
    out["model-with-whisper"] = voice.stt_command()
    out["whisper-run"] = _outcome(lambda: voice.transcribe(audio))
    out["whisper-args"] = log.read_text(encoding="utf-8").replace(str(ws.base), "<ws>")
    engine = ws.script(
        "fake-engine",
        f'/bin/echo "$1" >> "{log}.paths"\ncase "$2" in\n  fail) exit 3 ;;\n  empty) /bin/echo "[BLANK_AUDIO] [music]" ; exit 0 ;;\nesac\n/bin/echo "[00:00] hello  [BLANK_AUDIO] world"\n/bin/echo \'second  line\'\n',
    )
    _settings(ws, {"stt_command": f"'{engine}' {{audio}} ok", "stt_model": "/ignored.bin"})
    out["command-setting"] = voice.stt_command().replace(str(ws.base), "<ws>")
    out["wav-run"] = _outcome(lambda: voice.transcribe(audio))
    out["missing-audio"] = _outcome(lambda: voice.transcribe(ws.base / "nope.wav"))
    _settings(ws, {"stt_command": f"{engine} {{audio}} fail"})
    out["engine-fails"] = _outcome(lambda: voice.transcribe(audio))
    _settings(ws, {"stt_command": f"{engine} {{audio}} empty"})
    out["engine-empty"] = _outcome(lambda: voice.transcribe(audio))
    _settings(ws, {"stt_command": "rust23-missing-engine {audio}"})
    out["engine-missing"] = _outcome(lambda: voice.transcribe(audio))
    _settings(ws, {"stt_command": f"{engine} {{audio}} ok"})
    out["ogg-no-ffmpeg"] = _outcome(lambda: voice.transcribe(ogg))
    ws.script("ffmpeg", f'/bin/echo "$@" > "{log}.ffmpeg"\nfor last; do :; done\n/bin/cp "{audio}" "$last"\n')
    out["ogg-with-ffmpeg"] = _outcome(lambda: voice.transcribe(ogg))
    out["ffmpeg-args"] = (log.parent / (log.name + ".ffmpeg")).read_text(encoding="utf-8").replace(str(ws.base), "<ws>")
    ws.script("ffmpeg", "exit 1\n")
    out["ogg-ffmpeg-fails"] = _outcome(lambda: voice.transcribe(ogg))
    _settings(ws, {"stt_command": f"{VOICE / 'fake-stt.sh'} {{audio}}"})
    os.environ["PATH"] = f"{ws.bin}:/usr/bin:/bin"
    out["fixture-engine"] = _outcome(lambda: voice.transcribe(VOICE / "behavioral-answer.wav"))
    _settings(ws, {"stt_command": "'unbalanced {audio}"})
    out["unbalanced-quote"] = _outcome(lambda: voice.transcribe(audio))
    paths_seen = (log.parent / (log.name + ".paths")).read_text(encoding="utf-8").splitlines()
    tmp = Path(os.environ.get("TMPDIR") or "/tmp")
    out["engine-paths"] = [
        {
            "name": Path(p).name,
            "original": p == str(audio) or p == str(ogg),
            "temp": Path(p).parent.name.startswith("careerbrew-voice-"),
            "retained": Path(p).exists() and Path(p).parent.name.startswith("careerbrew-voice-"),
        }
        for p in paths_seen
    ]
    out["temp-dirs-left"] = len([p for p in tmp.iterdir() if p.name.startswith("careerbrew-voice-")]) if tmp.exists() else 0
    return out


def _voice_answer(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "voice-answer")
    store.write_text(ws.root / "sensitive-terms.txt", CANARY + "\n")
    cue_rows = [
        voice.cues({"text": "Named a trade-off and the alternatives, about which there were options"}),
        voice.cues({"text": "ignored", "cues": ["STAR", "p99", 5]}),
        voice.cues({"text": "Short words only here"}),
    ]
    said = {
        "boundary": voice._says("we cut latency by 40%", "40%"),
        "inside-word": voice._says("restarted", "star"),
        "word": voice._says("the star method", "star"),
        "symbol": voice._says("o(n^2) time", "o(n^2)"),
    }
    typed_answer = "Clarify edge cases first. Brute force is O(n^2), two pointers is O(n)."
    grades = {
        "arrays": voice.grade("arrays-two-pointers-1", typed_answer),
        "behavioral": voice.grade("beh-impact-1", (VOICE / "behavioral-answer.txt").read_text(encoding="utf-8")),
        "unknown": _outcome(lambda: voice.grade("rust23-unknown", "x")),
    }
    _settings(ws, {"stt_command": f"{VOICE / 'fake-stt.sh'} {{audio}}"})
    os.environ["PATH"] = f"{ws.bin}:/usr/bin:/bin"
    typed = voice.answer("arrays-two-pointers-1", text=typed_answer, seconds=42)
    spoken = voice.answer("beh-impact-1", audio=VOICE / "behavioral-answer.wav")
    neither = _outcome(lambda: voice.answer("beh-impact-1", text="   "))
    for row in (typed, spoken):
        row["row"].pop("at", None)
    printed = {}
    for label, ns in {
        "typed": Namespace(item="arrays-two-pointers-1", text=typed_answer, audio=None, seconds=None, aloud=False),
        "audio": Namespace(item="beh-impact-1", text=None, audio=str(VOICE / "behavioral-answer.wav"), seconds=None, aloud=False),
        "unknown": Namespace(item="rust23-unknown", text="x", audio=None, seconds=None, aloud=False),
        "canary": Namespace(item="arrays-two-pointers-1", text=f"{CANARY} clarify edge cases", audio=None, seconds=None, aloud=False),
    }.items():
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = cli.cmd_drill_speak(ns)
        printed[label] = {"code": code, "stdout": buffer.getvalue()}
    _settings(ws, {})
    printed["no-engine"] = {}
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer):
        code = cli.cmd_drill_speak(Namespace(item="beh-impact-1", text=None, audio=str(VOICE / "behavioral-answer.wav"), seconds=None, aloud=False))
    printed["no-engine"] = {"code": code, "stdout": buffer.getvalue()}
    return {"cues": cue_rows, "said": said, "grades": grades, "typed": typed, "spoken": spoken, "neither": neither, "printed": printed, "events": _events(ws)}


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust23-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "routes-config": _routes_config,
        "allowed-routes": _allowed_routes,
        "complete-routing": _complete_routing,
        "judge-text": _judge_text,
        "judge-cache": _judge_cache,
        "http-transports": _http_transports,
        "labels": _labels,
        "agreement-run": _agreement_run,
        "voice-transcribe": _voice_transcribe,
        "voice-answer": _voice_answer,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True) + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
