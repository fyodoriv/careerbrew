"""Eval runner: deterministic graders, plus the LLM judge with EVALS_LLM=1.

1. Structure: every skill has a SKILL.md with `name`, a description that says
   when to use it ("Use when") and when not ("Don't use").
2. Coverage: every skill has at least one `should: act` and one `should: refrain` case.
3. CLI cases (`cli:` + `expect_regex:`) run the real CLI on a fresh synthetic data
   root and must match.
4. Owner labels (evals/labels.jsonl) are well formed and name existing cases.
LLM-judged cases (`input:` + `expect:`) are listed; with EVALS_LLM=1 the judge on the local
route answers and grades them and checks itself against the owner's labels
(src/careerbrew/judge.py). The owner labels answers with:
  evals/run.sh show [case]               print a case, its criteria, and its answer
  evals/run.sh label <case> pass|fail    record your verdict on that answer
  evals/run.sh agreement                 check the judge against your labels only
"""

from __future__ import annotations

import argparse
import io
import os
import re
import sys
import tempfile
from collections.abc import Hashable
from contextlib import redirect_stdout
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]


class _CaseLoader(yaml.SafeLoader):
    def construct_mapping(self, node, deep=False):
        seen = set()
        merge_key = object()
        for key_node, _value_node in node.value:
            key = merge_key if key_node.tag == "tag:yaml.org,2002:merge" else self.construct_object(key_node, deep=deep)
            if not isinstance(key, Hashable):
                raise yaml.constructor.ConstructorError("while constructing a mapping", node.start_mark, "found unhashable key", key_node.start_mark)
            if key in seen:
                raise yaml.constructor.ConstructorError("while constructing a mapping", node.start_mark, "found duplicate key", key_node.start_mark)
            seen.add(key)
        return super().construct_mapping(node, deep=deep)


def load_cases() -> list[dict]:
    from careerbrew import judge

    cases = []
    folder = judge.repo_directory("evals", "cases")
    for f in sorted(folder.glob("*.yaml")):
        stream = io.StringIO(judge._read_resolved(judge.source_path(f, folder)))
        stream.name = str(f)
        cases.extend(c for c in yaml.load_all(stream, Loader=_CaseLoader) if c)
    return cases


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="evals/run.sh", description="Run the evals (EVALS_LLM=1 adds the LLM judge), or label answers for the judge.")
    sub = ap.add_subparsers(dest="cmd")
    sp = sub.add_parser("show", help="print a case, its criteria, and its answer (default: the next unlabeled case)")
    sp.add_argument("case", nargs="?")
    lp = sub.add_parser("label", help="record your verdict on the answer `show` printed, in evals/labels.jsonl")
    lp.add_argument("case")
    lp.add_argument("verdict", choices=["pass", "fail"])
    sub.add_parser("agreement", help="check the judge against your labels only")
    args = ap.parse_args(argv)
    return owner(args) if args.cmd else run_all()


def owner(args: argparse.Namespace) -> int:
    """show, label, agreement: the owner's side of the LLM judge."""
    from careerbrew import judge

    try:
        cases = load_cases()
        labels, problems = judge.read_labels(skip_case=getattr(args, "case", None))  # `show`/`label <case>` relabel that case's broken line
    except judge.LLMError as exc:
        print(f"FAIL  judge: {exc}")
        return 1
    problems += judge.check_labels(labels, cases)
    if problems:
        for p in problems:
            print(f"FAIL  {p}")
        return 1
    by_id = {c["id"]: c for c in judge.llm_cases(cases)}
    if getattr(args, "case", None) and args.case not in by_id:
        print(f"FAIL  {args.case} is not an LLM-judged case; `evals/run.sh show` prints the next one to label")
        return 2
    try:
        j = judge.Judge.from_config()
        if args.cmd == "agreement":
            for line in judge.agreement_lines(judge.agreement(j, labels, cases), j):
                print(line)
            return 0
        if args.cmd == "show":
            case = by_id[args.case] if args.case else judge.next_unlabeled(labels, cases)
            if case is None:
                print(f"every LLM-judged case is labeled ({len(labels)}); run `evals/run.sh agreement`")
                return 0
            print(judge.show_text(case, j.answer(case), j, labels, cases))
            return 0
        answer = j.cached_answer(by_id[args.case])
        if answer is None:
            print(f"FAIL  no answer shown yet for {args.case}; run `evals/run.sh show {args.case}` first, read it, then label it")
            return 1
        n = judge.add_label(by_id[args.case], answer, args.verdict)
        print(f"labeled {args.case} {args.verdict} in {judge.labels_path()} ({n}/{judge.MIN_LABELS} labels); commit the file")
        return 0
    except judge.LLMError as e:
        print(f"FAIL  judge: {e}")
        return 1


def run_all() -> int:
    from careerbrew import judge

    fails: list[str] = []
    try:
        folder = judge.repo_directory("skills")
        skills = {p.parent.name: p for p in folder.glob("*/SKILL.md")}
        skill_contents = {name: judge.skill_text(name) for name in skills}
        cases = load_cases()
    except judge.LLMError as exc:
        print(f"FAIL  judge: {exc}")
        return 1
    for name, path in skills.items():
        text = skill_contents[name]
        front = yaml.safe_load(text.split("---")[1]) if text.startswith("---") else {}
        desc = (front or {}).get("description", "")
        if (front or {}).get("name") != name:
            fails.append(f"{name}: frontmatter name mismatch")
        if "Use when" not in desc or "Don't use" not in desc:
            fails.append(f"{name}: description needs 'Use when' and 'Don't use'")
        body = text.split("---", 2)[2] if text.startswith("---") else text
        first_step = re.search(r"(?m)^(\d+\.|First,)", body)
        if name != "careerbrew-best-practices" and not re.search(r"load `careerbrew-best-practices`", body[: (first_step.end() + 200) if first_step else 400], re.IGNORECASE):
            fails.append(f"{name}: must load `careerbrew-best-practices` before its first step")
        if not re.search(r"(?m)^## Iterate\s*$", body) or body.rstrip().rfind("## Iterate") < body.rstrip().rfind("\n## ") - 1:
            fails.append(f"{name}: must end with an `## Iterate` section")
        if not (path.parent / "LESSONS.md").exists():
            fails.append(f"{name}: needs LESSONS.md for its Iterate step")

    for name in skills:
        mine = [c for c in cases if c.get("skill") == name]
        for kind in ("act", "refrain"):
            if not any(c.get("should") == kind for c in mine):
                fails.append(f"{name}: no `should: {kind}` case")
    unknown = {c.get("skill") for c in cases} - set(skills)
    fails.extend(f"case for unknown skill {u}" for u in sorted(unknown))

    from careerbrew.cli import main as cli

    ran = llm = 0
    for c in cases:
        if "cli" not in c:
            llm += 1
            continue
        with tempfile.TemporaryDirectory() as tmp:
            previous = dict(os.environ)
            try:
                fixture_env = {
                    "CAREERBREW_HOME": f"{tmp}/data",
                    "CAREERBREW_EVENTS": "off",
                    "CAREERBREW_LLM": "off",
                    "CAREERBREW_KEY_FILE": f"{tmp}/k",
                    "CAREERBREW_BACKUP_DIRS": f"{tmp}/b",
                    "CAREERBREW_PAIR_KEY_FILE": f"{tmp}/p",
                }
                overrides = c.get("env") or {}
                if set(overrides) & fixture_env.keys():
                    fails.append(f"{c['id']}: case cannot override synthetic safety settings")
                    continue
                os.environ.update(fixture_env)
                os.environ.update({k: str(v) for k, v in overrides.items()})
                buf = io.StringIO()
                with redirect_stdout(buf):
                    if c.get("setup"):
                        cli(c["setup"])
                    cli(c["cli"])
                ran += 1
                if not re.search(c["expect_regex"], buf.getvalue()):
                    fails.append(f"{c['id']}: output did not match /{c['expect_regex']}/: {buf.getvalue()[:200]!r}")
            finally:
                os.environ.clear()
                os.environ.update(previous)
    try:
        labels, problems = judge.read_labels()
    except judge.LLMError as exc:
        print(f"FAIL  judge: {exc}")
        return 1
    fails += problems + judge.check_labels(labels, cases)
    judged = False
    if os.environ.get("EVALS_LLM") == "1":
        try:
            lines, llm_fails = judge.run(cases, judge.Judge.from_config(), labels, progress=lambda s: print(s, file=sys.stderr, flush=True))
            print("\n".join(lines))
            fails += llm_fails
            judged = True
        except judge.LLMError as e:
            fails.append(f"judge: {e}")
    else:
        print(f"labels: {len(judge.current_labels(labels, cases))}/{judge.MIN_LABELS} owner labels; EVALS_LLM=1 runs the judge on the local route")
    for f in fails:
        print(f"FAIL  {f}")
    print(f"evals: {len(skills)} skills, {ran} CLI cases run, {llm} LLM-judged cases {'judged' if judged else 'listed'}, {len(fails)} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
