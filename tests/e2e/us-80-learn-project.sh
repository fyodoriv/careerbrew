#!/usr/bin/env bash
# US-80: learn a project across days; every taught concept is scheduled, nothing lands in the learned repo.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-10-26T09:00+00:00"
# Error case first: an unknown project names its fix.
expect "$(cb learn status toy 2>&1 || true)" "careerbrew learn start toy" "error case: unknown project names the fix"
repo="$SCRATCH/toyrepo"
mkdir -p "$repo/src" && printf '# Toy\nA queue and a worker.\n' >"$repo/README.md" && printf 'class Queue: ...\n' >"$repo/src/queue.py"
git init -q "$repo"
before="$(cd "$repo" && find . -path ./.git -prune -o -type f -print | sort)"
printf 'Deploy with make ship.\n' >"$SCRATCH/runbook.md"
expect "$(cb learn start toy --repo "$repo" --doc "runbook:$SCRATCH/runbook.md" --consume)" "2 source(s) staged" "sources staged"
[ ! -e "$SCRATCH/runbook.md" ] || fail "consume left the plaintext doc"
refute "$(cat "$CAREERBREW_HOME"/learn/toy/inputs/doc/runbook.md)" "make ship" "fetched text is encrypted at rest"
expect "$(cb learn start toy --repo "$repo")" "changed: none" "unchanged sources are skipped"
printf '# Core\nQueue then worker.\n' >"$SCRATCH/note.md"
expect "$(cb learn note toy 01-core.md --set "$SCRATCH/note.md" --consume)" "saved note 01-core.md" "note saved"
cat >"$SCRATCH/bad.yaml" <<'YAML'
concepts:
  - {id: a, section: s, title: A, prereqs: [a], cites: [repo:toyrepo/missing.py], questions: [{bloom: remember, prompt: P, answer: A}]}
YAML
if cb learn concepts toy "$SCRATCH/bad.yaml" --check >"$SCRATCH/o" 2>&1; then fail "a bad concepts file passed"; fi
expect "$(cat "$SCRATCH/o")" "citation repo:toyrepo/missing.py" "error case: uncited concept refused"
expect "$(cat "$SCRATCH/o")" "cycle" "error case: prerequisite cycle refused"
cat >"$SCRATCH/concepts.yaml" <<'YAML'
concepts:
  - {id: queue, section: core, title: The queue, cites: [repo:toyrepo/src/queue.py], questions: [{bloom: understand, prompt: "Why a queue?", answer: Decoupling.}, {bloom: apply, prompt: "Enqueue twice?", answer: Runs twice.}]}
  - {id: worker, section: core, title: The worker, prereqs: [queue], cites: [note:01-core.md], questions: [{bloom: understand, prompt: "What pulls jobs?", answer: The worker.}]}
  - {id: deploy, section: ops, title: Deploys, cites: [doc:runbook], questions: [{bloom: apply, prompt: "How to ship?", answer: make ship.}]}
YAML
expect "$(cb learn concepts toy "$SCRATCH/concepts.yaml" --consume)" "3 concept(s)" "concepts imported"
expect "$(cb learn next toy)" "pretest: The queue" "a new section starts with a pretest"
cb learn answer toy queue --mode pretest --result dontknow --confidence low --level understand >/dev/null
cb learn answer toy queue --mode teach --result correct --confidence high --level understand >/dev/null
expect "$(cb learn answer toy queue --result correct --confidence medium --level apply)" "mastered (retests at +1 and +7 days)" "two confident correct answers master a concept"
expect "$(cb learn answer toy worker --mode teach --result wrong --confidence high --level understand)" "misconception" "high-confidence miss is flagged"
expect "$(cb learn answer toy nope --result correct --confidence high --level apply 2>&1 || true)" "careerbrew learn status toy" "error case: unknown concept names the fix"
share="$(cb learn status toy --json | "$ROOT/.venv/bin/python" -c 'import json,sys; print(json.load(sys.stdin)["scheduled_share"])')"
[ "$share" = "1.0" ] || fail "scheduled_share is $share, want 1.0"
expect "$(cb learn list)" "toy: 1/3 mastered" "list shows progress"
expect "$(CAREERBREW_NOW="2026-10-27T09:05+00:00" cb learn next toy)" "retest: The queue" "the +1 day retest comes first"
[ "$before" = "$(cd "$repo" && find . -path ./.git -prune -o -type f -print | sort)" ] || fail "learn wrote into the learned repo"
event_seen learn.start
event_seen learn.concepts
event_seen learn.answer
echo "ok US-80 learn project"
