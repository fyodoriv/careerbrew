#!/usr/bin/env bash
# US-66: LLM-judged evals on the local route, checked against the owner's labels.
# A fake OpenAI-compatible judge (tests/e2e/fake-judge.py) stands in for the model: no download, no network.
source "$(dirname "$0")/lib.sh"
export EVALS_CACHE="$SCRATCH/evals-cache" EVALS_LABELS="$SCRATCH/labels.jsonl" CAREERBREW_NOW=2026-10-22T10:00
unset EVALS_LLM
"$ROOT/.venv/bin/python" "$ROOT/tests/e2e/fake-judge.py" "$SCRATCH/port" "$SCRATCH/requests.log" &
judge_pid=$!
trap 'kill "$judge_pid" 2>/dev/null; rm -rf "$SCRATCH"' EXIT
for _ in $(seq 100); do [ -s "$SCRATCH/port" ] && break; sleep 0.1; done
[ -s "$SCRATCH/port" ] || fail "the fake judge listens"
export EVALS_LLM_URL="http://127.0.0.1:$(cat "$SCRATCH/port")/v1"
ev() { (cd "$ROOT" && evals/run.sh "$@"); }
n="$(cd "$ROOT" && .venv/bin/python -c 'import glob, yaml; print(sum(1 for f in glob.glob("evals/cases/*.yaml") for c in yaml.safe_load_all(open(f)) if c and "cli" not in c))')"
[ "$n" -ge 20 ] || fail "at least 20 LLM-judged cases exist to label (found $n)"

# Happy: EVALS_LLM=1 answers and judges every input/expect case on the local route, one criterion per call.
out="$(EVALS_LLM=1 ev 2>/dev/null)" || { printf '%s\n' "$out" >&2; fail "EVALS_LLM=1 evals/run.sh exits 0 while the judge is not trusted yet"; }
expect "$out" "llm: $n case(s) answered on route local" "every input/expect case is answered on the local route"
expect "$out" "$n LLM-judged cases judged, 0 failure(s)" "every case is judged; an untrusted judge fails nothing"
expect "$out" "start-session-proposes-one-step: judged fail (criterion 2: FAIL, fake judge) [judge not trusted yet]" "a judged failure names its criterion"
expect "$out" "decision: COLLECT: 0/20 labels" "with no labels the judge is not trusted"
crit="$(cd "$ROOT" && .venv/bin/python -c 'import glob, yaml; print(sum(len(c["expect"]) for f in glob.glob("evals/cases/*.yaml") for c in yaml.safe_load_all(open(f)) if c and "cli" not in c))')"
[ "$(grep -c ' judge ' "$SCRATCH/requests.log")" = "$crit" ] || fail "one judge call per criterion ($crit)"
# Answers are cached by their inputs (the skill and the prompt), so two cases that share both share one answer call.
prompts="$(cd "$ROOT" && .venv/bin/python -c 'import glob, yaml; print(len({(c.get("skill"), c["input"]) for f in glob.glob("evals/cases/*.yaml") for c in yaml.safe_load_all(open(f)) if c and "cli" not in c}))')"
[ "$(grep -c ' answer ' "$SCRATCH/requests.log")" = "$prompts" ] || fail "one answer call per distinct prompt ($prompts)"

# The owner labels: show prints the next unlabeled case and its answer, label records the verdict.
out="$(ev show)"
expect "$out" "case: start-session-proposes-one-step" "show starts with the first unlabeled case"
expect "$out" 'I would run `careerbrew review`' "show prints the answer to label"
refute "$out" "<think>" "the model's thinking is not part of the answer"
expect "$out" "label it: evals/run.sh label start-session-proposes-one-step pass" "show prints the label command"
expect "$(ev label start-session-proposes-one-step pass)" "(1/20 labels)" "label records the owner's verdict"
ev show start-session-refrains-on-trivia >/dev/null
ev label start-session-refrains-on-trivia pass >/dev/null
third="$(cd "$ROOT" && .venv/bin/python -c 'import glob, yaml; print([c["id"] for f in sorted(glob.glob("evals/cases/*.yaml")) for c in yaml.safe_load_all(open(f)) if c and "cli" not in c][2])')"
expect "$(ev show)" "case: $third" "show moves on to the next unlabeled case"
ev label "$(ev show | sed -n 's/^case: \([^ ]*\).*/\1/p')" pass >/dev/null
[ "$(wc -l < "$EVALS_LABELS" | tr -d ' ')" = 3 ] || fail "three labels, one line each"
grep -q '"labeled": "2026-10-22"' "$EVALS_LABELS" || fail "a label carries its date from the pinned clock"

# Agreement: the judge grades each labeled answer; the fake judge fails the first one, the owner passed it.
before="$(wc -l < "$SCRATCH/requests.log")"
out="$(ev agreement)"
expect "$out" "judge agreement: 2/3 labels agree (67%) on route local" "agreement against the owner's labels"
expect "$out" "  disagree  start-session-proposes-one-step: owner pass, judge fail" "each disagreement is listed"
expect "$out" "decision: COLLECT: 3/20 labels" "fewer than 20 labels: keep labeling"
[ "$(wc -l < "$SCRATCH/requests.log")" = "$before" ] || fail "labeled answers were judged already: no model call (cache by input hash)"

# Errors: a judge that does not answer names the fix; an unknown case is refused; an edited label fails the evals.
out="$(EVALS_CACHE="$SCRATCH/empty-cache" EVALS_LLM_URL="http://127.0.0.1:9/v1" ev agreement)" && fail "agreement fails when nothing answers"
expect "$out" "nothing answers at http://127.0.0.1:9/v1" "the error names the URL"
expect "$out" '`ollama serve`' "the error names the fix"
out="$(ev label no-such-case pass)" && fail "an unknown case is refused"
expect "$out" "no-such-case is not an LLM-judged case" "the refusal names the case"
out="$(EVALS_CACHE="$SCRATCH/empty-cache" ev label job-hunt-refrains-on-corporate-machine pass)" && fail "label needs an answer the owner was shown"
expect "$out" "run \`evals/run.sh show job-hunt-refrains-on-corporate-machine\` first" "label says to show the answer first"
"$ROOT/.venv/bin/python" - "$EVALS_LABELS" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1])]
rows[0]["answer"] += " (edited)"
open(sys.argv[1], "w").write("".join(json.dumps(r) + "\n" for r in rows))
PY
out="$(ev)" && fail "an edited label fails the evals"
expect "$out" 'the answer changed after it was labeled; label it again: `evals/run.sh show start-session-proposes-one-step`' "the evals name the edited label and its fix"
# The named fix works: show the case again, label it again, and the evals pass.
out="$(ev show start-session-proposes-one-step)" || { printf '%s\n' "$out" >&2; fail "show runs on the case whose label was edited"; }
expect "$out" "label it: evals/run.sh label start-session-proposes-one-step pass" "show prints the case to label again"
expect "$(ev label start-session-proposes-one-step pass)" "(3/20 labels)" "label replaces the edited label"
out="$(ev)" || { printf '%s\n' "$out" >&2; fail "the evals pass after the named fix"; }
# Eval inputs and outputs must not read or write inside the synthetic data root.
mkdir -p "$CAREERBREW_HOME/work"
printf 'synthetic private canary\n' > "$CAREERBREW_HOME/work/labels.jsonl"
out="$(EVALS_LABELS="$CAREERBREW_HOME/work/labels.jsonl" ev show)" && fail "protected labels are refused"
expect "$out" "data root" "the protected label refusal names the boundary"
refute "$out" "synthetic private canary" "the refusal hides label content"
before="$(wc -l < "$SCRATCH/requests.log")"
out="$(EVALS_CACHE="$CAREERBREW_HOME/work/cache" ev show start-session-proposes-one-step)" && fail "protected cache is refused"
expect "$out" "data root" "the protected cache refusal names the boundary"
[ "$(wc -l < "$SCRATCH/requests.log")" = "$before" ] || fail "protected cache is refused before a model call"
[ ! -e "$CAREERBREW_HOME/work/cache" ] || fail "protected cache is not created"
# A case symlink into the synthetic data root is refused before its content is read.
mkdir -p "$SCRATCH/eval-repo/evals/cases" "$SCRATCH/eval-repo/skills"
printf 'id: synthetic\ninput: synthetic-private-canary\n' > "$CAREERBREW_HOME/work/case.yaml"
ln -s "$CAREERBREW_HOME/work/case.yaml" "$SCRATCH/eval-repo/evals/cases/case.yaml"
out="$("$ROOT/.venv/bin/python" - "$ROOT/evals/run.py" "$SCRATCH/eval-repo" <<'PYCASE'
import importlib.util
import sys
from pathlib import Path
from careerbrew import paths
spec = importlib.util.spec_from_file_location("synthetic_eval_runner", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.ROOT = Path(sys.argv[2])
paths.REPO_ROOT = module.ROOT
raise SystemExit(module.main(["show"]))
PYCASE
)" && fail "protected case source is refused"
expect "$out" "data root" "the protected source refusal names the boundary"
refute "$out" "synthetic-private-canary" "the refusal hides source content"
echo "ok US-66 llm evals"
