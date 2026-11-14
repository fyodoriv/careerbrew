#!/usr/bin/env bash
# The one gate. Agents run it before they say a change is done.
#   scripts/check.sh        full gate: docs, lint, types, unit tests, e2e (the hosted checks run it for every pull request)
#   scripts/check.sh fast   skips e2e (each commit; keep it under 10 seconds)
# Each step's time goes to .metrics/events.jsonl; scripts/metrics.sh reports it.
#
# A cache-eligible step that passed can be skipped while its inputs are unchanged,
# except `leaks`, format, lint, types, unit, web-types, e2e and Rust checks, whose inputs are not fully keyed
# (hash of the content of tracked and untracked files, like the nx cache). A
# step's inputs are every file, or the git pathspecs `inputs_of` names for it,
# like an nx target's inputs; `scripts/check.sh inputs <step>...` prints them.
# A new commit also re-runs `secrets`.
# NO_CACHE=1 forces a full run (do it after a tool upgrade). CHECK_OFFLINE=1
# skips steps that need the network.
# The cache lives in .metrics/cache/<step>, one file per step, per checkout.
set -euo pipefail
cd "$(dirname "$0")/.."
# A git hook exports GIT_DIR and friends; tests that run git would then write to this repo.
unset $(git rev-parse --local-env-vars)
mode="${1:-full}"

# Step inputs, like nx target inputs: the git pathspecs a step reads; no line
# means every file. Leave a path out of a step only when no code, test, or story
# the step runs reads it: tests/unit/test_gate_inputs.py greps them and fails
# otherwise, so a cache hit is never a false green.
NOT_READ=(':(exclude)TASKS.md' ':(exclude)RECURRING.md' ':(exclude)docs/learnings/')  # notes no step below reads
inputs_of() { # inputs_of <step>: print its pathspecs, one per line (none: every file)
  case "$1" in
    # unit reads every file: test_no_conflict_markers.py scans each tracked file.
    format | lint | types | public-cards | card-code | exercises | web-* | e2e) printf '%s\n' "${NOT_READ[@]}" ;;
  esac
}
if [ "$mode" = inputs ]; then
  shift
  for s in "$@"; do printf '%s\t%s\n' "$s" "$(inputs_of "$s" | paste -sd ' ' -)"; done
  exit 0
fi
mkdir -p .metrics/cache
# A fresh worktree has no web/node_modules, and the unit tests that list Playwright specs need it (issue #221).
if [ -f web/package.json ] && [ ! -d web/node_modules ]; then
  echo "check.sh: web/node_modules is missing -> fix: scripts/setup.sh (or: cd web && npm ci && npm run build)" >&2
  exit 1
fi

key=""
if [ "${NO_CACHE:-0}" != 1 ]; then
  # The key hashes file content only (scripts/input-key.sh), so it is the same in
  # fast and full mode and before and after `git add` or `git commit`: a later
  # run reuses a green gate on the same files. On any error the
  # key is empty: no cache.
  key="$(scripts/input-key.sh)" || key=""
fi

input_sets=() input_keys=()
step_key() { # step_key <name>: set k to one step's cache key, from its inputs (memoized per set)
  local spec i
  spec="$(inputs_of "$1")"; k="$key"
  [ -n "$key" ] && [ -n "$spec" ] || return 0
  for i in ${input_sets[@]+"${!input_sets[@]}"}; do
    [ "${input_sets[$i]}" = "$spec" ] && { k="${input_keys[$i]}"; return 0; }
  done
  # The pathspecs hold no spaces. On any error the key is empty: no cache.
  # shellcheck disable=SC2086
  k="$(scripts/input-key.sh $spec)" || k=""
  input_sets+=("$spec"); input_keys+=("$k")
}
step() {
  # step <name> <command...>: run, time, and log one gate step; skip it on a cache hit.
  name="$1"; shift
  local k; step_key "$name"
  case "$name" in format | lint | types | unit | exercises | web-types | e2e | rust-fmt | rust-clippy | rust-test | rust-inventory) k="" ;; esac
  if [ -n "$k" ] && [ "$(cat ".metrics/cache/$name" 2>/dev/null)" = "$k" ]; then
    printf '{"ts":"%s","event":"check.step","name":"%s","seconds":0,"ok":true,"cached":true}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$name" >> .metrics/events.jsonl
    echo "check.sh: $name cached (inputs unchanged)"
    return
  fi
  start=$(date +%s)
  if "$@"; then okv=true; else okv=false; fi
  secs=$(( $(date +%s) - start ))
  printf '{"ts":"%s","event":"check.step","name":"%s","seconds":%s,"ok":%s}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$name" "$secs" "$okv" >> .metrics/events.jsonl
  [ "$okv" = true ] || { echo "check.sh: step '$name' failed" >&2; exit 1; }
  [ -z "$k" ] || echo "$k" > ".metrics/cache/$name"
}

step docs scripts/check-docs.sh
step learnings uv run --quiet python scripts/learnings-table.py --check
# Leak terms can change outside the tracked tree. Always scan the current terms.
content_key="$key"; key=""
step leaks scripts/leak-check.sh
key="$content_key"
step sources uv run --quiet python scripts/check-sources.py
step stories uv run --quiet python scripts/check-stories.py
if command -v gitleaks >/dev/null 2>&1 && git rev-parse -q --verify HEAD >/dev/null; then
  # gitleaks reads commits, not files: key it by HEAD too, so every new commit is scanned.
  content_key="$key"; key="${key:+$key-$(git rev-parse HEAD)}"
  step secrets gitleaks git --no-banner --redact -l warn --log-opts=-20
  key="$content_key"
fi
if [ -f TASKS.md ] && command -v npx >/dev/null 2>&1 && [ "${CHECK_OFFLINE:-0}" != 1 ]; then
  if npx --no-install @tasks-md/lint --help >/dev/null 2>&1 || npm view @tasks-md/lint version >/dev/null 2>&1; then
    step tasks npx -y @tasks-md/lint TASKS.md
  else
    echo "check.sh: tasks lint skipped (offline and not cached)"
  fi
fi

if [ -f package.json ]; then
  pm=npm; [ -f pnpm-lock.yaml ] && pm=pnpm
  step lint "$pm" run --if-present lint
  step types "$pm" run --if-present typecheck
  step unit "$pm" test
fi
if [ -f pyproject.toml ]; then
  step format uv run ruff format --check .
  step lint uv run ruff check .
  if uv run --quiet pyright --version >/dev/null 2>&1; then step types uv run pyright; fi
  # pytest-xdist runs the unit tests on every CPU; UNIT_JOBS=0 runs them one by one.
  step unit uv run pytest -q -n "${UNIT_JOBS:-auto}"
  step public-cards env -u CAREERBREW_PUBLIC_CARDS uv run careerbrew cards public-check
  step card-code uv run python scripts/check-card-code.py
  step exercises env -u CAREERBREW_EXERCISES uv run careerbrew exercises check
fi
if [ -f Cargo.toml ]; then
  step rust-fmt cargo fmt --all --check
  step rust-clippy cargo clippy --all-targets -- -D warnings
  step rust-test cargo test --all
  step rust-inventory cargo xtask migration inventory --check
fi

if [ -f web/package.json ]; then
  step web-types bash -c "cd web && npm run --silent typecheck"
  step web-lint bash -c "cd web && npm run --silent lint"
  step web-unit bash -c "cd web && npm run --silent test"
  step web-build bash -c "cd web && npm run --silent build >/dev/null"
fi
if [ "$mode" != fast ]; then step e2e scripts/e2e.sh; fi
if [ "${CHECK_EVALS:-0}" = 1 ]; then
  [ -x evals/run.sh ] || { echo "check.sh: CHECK_EVALS=1 but evals/run.sh is missing" >&2; exit 1; }
  step evals evals/run.sh
fi
echo "check.sh: $mode gate passed"
