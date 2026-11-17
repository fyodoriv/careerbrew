#!/bin/bash
# Test fixture for hooks/block-local-tests.sh. Run: bash hooks/block-local-tests.test.sh
set -uo pipefail
SCRIPT="$(cd "$(dirname "$0")" && pwd)/block-local-tests.sh"
FAIL=0
TMP="$(mktemp -d "${TMPDIR:-/tmp}/block-local-tests.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
# A fake careerbrew checkout (the two marker files) and two plain folders.
CHECKOUT="$TMP/careerbrew"
OTHER="$TMP/other"
LOOKALIKE="$TMP/lookalike"
mkdir -p "$CHECKOUT/scripts" "$CHECKOUT/web" "$OTHER" "$LOOKALIKE/scripts"
printf '[project]\nname = "careerbrew"\n' > "$CHECKOUT/pyproject.toml"
: > "$CHECKOUT/scripts/check.sh"
printf '[project]\nname = "other"\n' > "$LOOKALIKE/pyproject.toml"
: > "$LOOKALIKE/scripts/check.sh"
unset CAREERBREW_ALLOW_LOCAL_TESTS

check() { # expected-exit command [cwd] [env assignment]
  local input got
  input=$(python3 -c 'import json,sys; print(json.dumps({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":sys.argv[1]},"cwd":sys.argv[2]}))' "$2" "${3:-$CHECKOUT}")
  if [ -n "${4:-}" ]; then
    echo "$input" | env "$4" bash "$SCRIPT" >/dev/null 2>&1; got=$?
  else
    echo "$input" | bash "$SCRIPT" >/dev/null 2>&1; got=$?
  fi
  if [ "$got" != "$1" ]; then echo "FAIL: expected $1, got $got: $2 (cwd ${3:-checkout})"; FAIL=1; fi
}

# Direct test runs are blocked in a checkout.
for c in "pytest" "pytest tests/unit/test_x.py -q" "uv run pytest -q" "uv run --locked pytest tests" \
         "python -m pytest" "python3 -m pytest -x" ".venv/bin/pytest" "uv run python -m pytest" \
         "cargo test" "cargo nextest run" "cargo +1.100.0 test -p xtask" "cargo --locked test" \
         "scripts/check.sh" "scripts/check.sh fast" "bash scripts/check.sh fast" "./scripts/check.sh" \
         "NO_CACHE=1 scripts/check.sh" "CHECK_STEPS=e2e E2E_SHARD=1/2 scripts/check.sh" \
         "scripts/e2e.sh" "scripts/batch-gate.sh 1 2" "tests/e2e/us-01-setup.sh" "bash tests/e2e/us-40-x.sh" \
         "evals/run.sh" "EVALS_LLM=1 evals/run.sh" "uv run python scripts/tests_affected.py --run" \
         "heavy-run uv run pytest -q -n 8 tests/unit" "heavy-run --exclusive scripts/check.sh" \
         "cd web && npm test" "cd web && npm run test" "npm run visual" "npm run visual:update" \
         "npx vitest run" "npx playwright test" "uv run ruff check . && uv run pytest" "pytest | tail -5"; do
  check 2 "$c"
done

# Launchers: xargs, find -exec, npx, npm exec, pnpm dlx, yarn, bunx, uvx, bash -c, eval.
for c in "ls tests | xargs pytest" "xargs -n1 -I{} uv run pytest {}" "find tests -name 'test_*.py' -exec pytest {} +" \
         "find . -execdir cargo test \\;" "npm exec vitest" "npm exec -- vitest run" "npx -p @playwright/test playwright test" \
         "npm exec -c 'vitest run'" "pnpm dlx vitest run" "yarn vitest" "yarn test" "pnpm visual" "bunx vitest" \
         "bun test" "uvx pytest" "uvx --from pytest pytest" "npx tsx --test x.ts" "node --test x.mjs" \
         "bash -c 'uv run pytest'" "sh -c \"scripts/check.sh fast\"" "eval pytest" "eval 'cargo test'" \
         "echo \$(pytest -q)"; do
  check 2 "$c"
done

# A command that names the checkout folder is blocked from any session folder.
check 2 "cd $CHECKOUT && uv run pytest" "$OTHER"
check 2 "cargo test --manifest-path $CHECKOUT/Cargo.toml" "$OTHER"
# A subfolder of the checkout counts too.
check 2 "npm test" "$CHECKOUT/web"

# Allowed: lint, format, typecheck, build, read-only commands, one hook's own fixture.
for c in "uv run ruff check --fix ." "uv run ruff format ." "uv run pyright" "npm run typecheck" "npm run lint" \
         "npm run build" "cargo build" "cargo clippy" "cargo fmt --check" "rg pytest tests" "cat tests/unit/test_x.py" \
         "git commit -m 'run pytest'" "echo 'cargo test' | xargs echo" "vitest --version" "npx vitest list" \
         "scripts/check.sh inputs unit" "scripts/doctor.sh" "uv run python scripts/tests_affected.py" \
         "bash hooks/block-local-tests.test.sh" "gh pr checks 1 --watch" "uv run careerbrew actions --json" \
         "uv sync" "ls tests/e2e"; do
  check 0 "$c"
done

# Allowed: test runs outside a careerbrew checkout, or in a look-alike repo.
check 0 "uv run pytest" "$OTHER"
check 0 "scripts/check.sh" "$LOOKALIKE"

# Opt-out from the hook's own environment.
check 0 "uv run pytest" "$CHECKOUT" "CAREERBREW_ALLOW_LOCAL_TESTS=1"
# Setting it in the command text does not opt out.
check 2 "CAREERBREW_ALLOW_LOCAL_TESTS=1 uv run pytest"
check 2 "export CAREERBREW_ALLOW_LOCAL_TESTS=1; pytest"
check 2 "env CAREERBREW_ALLOW_LOCAL_TESTS=1 scripts/check.sh fast"

# The block message names hosted CI, gh pr checks and the opt-out.
msg=$(python3 -c 'import json; print(json.dumps({"tool_input":{"command":"pytest"}}))' \
  | (cd "$CHECKOUT" && bash "$SCRIPT" 2>&1 >/dev/null))
for want in "hosted CI" "gh pr checks <pr>" "CAREERBREW_ALLOW_LOCAL_TESTS=1"; do
  case "$msg" in *"$want"*) ;; *) echo "FAIL: block message lacks: $want"; FAIL=1 ;; esac
done

[ "$FAIL" = 0 ] && echo "block-local-tests: all passed"
exit $FAIL
