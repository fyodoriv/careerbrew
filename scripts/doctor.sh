#!/usr/bin/env bash
# Health check. One line per check: "ok", or "FAIL" plus the exact fix.
# Exit 0 only when every check passes. Content-free: never prints user data.
set -uo pipefail
cd "$(dirname "$0")/.."

fails=0
ok() { echo "ok    $1"; }
fail() { echo "FAIL  $1 -> fix: $2"; fails=$((fails + 1)); }
need() { if command -v "$1" >/dev/null 2>&1; then ok "$1 installed"; else fail "$1 missing" "$2"; fi; }

need git "install git"
if command -v node >/dev/null 2>&1; then ok "node installed (web app)"; else fail "node missing (web app)" "install Node.js 20+ (brew install node)"; fi
[ -f web/dist/index.html ] && ok "web app built" || fail "web app not built" "cd web && npm ci && npm run build"
if [ -d web/node_modules ]; then
  [ -f web/node_modules/remark-parse/package.json ] && [ -f web/node_modules/unified/package.json ] && ok "PM copy Markdown parser installed" || fail "PM copy Markdown parser missing" "cd web && npm ci"
fi
# The browser suite's comfort budget (web/e2e/comfort.spec.ts) needs axe-core. A note, not a failure: the app runs without it.
if [ -d web/node_modules/@axe-core/playwright ]; then ok "browser test deps installed (axe-core)"; else echo "note  browser test deps missing -> fix: cd web && npm ci (npx playwright test needs them)"; fi
need gitleaks "brew install gitleaks"
if command -v lsof >/dev/null 2>&1; then ok "lsof installed (careerbrew open finds a careerbrew server on its port that nothing tracks)"; else ok "lsof missing: careerbrew open cannot stop an untracked careerbrew server on its port (optional: install lsof)"; fi
[ -f package.json ] && need node "mise use node@lts"
[ -f pnpm-lock.yaml ] && need pnpm "corepack enable"
[ -f pyproject.toml ] && need uv "curl -LsSf https://astral.sh/uv/install.sh | sh"
# This doctor invocation owns the readiness decision.
rust_ready=0
if [ -f Cargo.toml ]; then
  source scripts/rust-readiness.sh
  if rust_readiness; then rust_ready=1; else fails=$((fails + 1)); fi
fi

# Direct native dependencies are checked without compiling or fetching.
if [ -f Cargo.toml ] && [ "$rust_ready" = 1 ]; then
  native_py=python3
  [ -x .venv/bin/python ] && native_py=.venv/bin/python
  if metadata="$(RUSTUP_AUTO_INSTALL=0 "$RUST_READY_BIN/cargo" metadata --locked --offline --no-deps --format-version 1 2>/dev/null)" &&
    printf '%s' "$metadata" | "$native_py" -c '
import json, sys
try:
    packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "careerbrew-xtask"]
    deps = [d for d in packages[0]["dependencies"] if d["name"] == "serde"] if len(packages) == 1 else []
    valid = len(deps) == 1 and deps[0]["req"] == "=1.0.229" and isinstance(deps[0]["features"], list) and "derive" in deps[0]["features"] and deps[0].get("kind") is None and deps[0].get("target") is None
except (KeyError, TypeError, ValueError):
    valid = False
sys.exit(0 if valid else 1)
' 2>/dev/null; then
    ok "native Serde derive dependency (locked, offline)"
  else
    fail "native Serde derive dependency unavailable or incompatible" 'restore xtask serde =1.0.229 with derive and Cargo.lock; cargo fetch --locked'
  fi
fi

if [ -f .env.example ]; then
  for key in $(grep -E '^[A-Z][A-Z0-9_]*=.*# required' .env.example | cut -d= -f1); do
    if grep -qE "^$key=.+" .env 2>/dev/null || [ -n "${!key:-}" ]; then ok "env $key set"
    else fail "env $key empty" "set it in .env (see .env.example)"; fi
  done
fi
# Commands and skills call `careerbrew` and read `$(careerbrew where)` (issue #304). A note in a linked worktree.
if command -v careerbrew >/dev/null 2>&1 && [ -d "$(careerbrew where 2>/dev/null)/skills" ]; then ok "careerbrew command on PATH ($(careerbrew where))"
elif [ -f .git ]; then echo "note  careerbrew command missing (fine in a linked worktree; the main checkout installs it)"
else fail "careerbrew command missing (commands and skills need it)" "scripts/install-launcher.sh, and add ~/.local/bin to PATH"; fi
[ -d .metrics ] && ok ".metrics/ exists" || fail ".metrics/ missing" "scripts/setup.sh"
[ -d private ] && ok "private/ exists (ignored by git)" || fail "private/ missing" "scripts/setup.sh"
[ -f config/models.yaml ] && ok "config/models.yaml present" || fail "config/models.yaml missing" "restore it from git"
if [ -f pyproject.toml ]; then
  uv run --quiet python -c "import careerbrew, fsrs, cryptography, yaml" 2>/dev/null && ok "python deps import" || fail "python deps missing" "uv sync"
  uv run --quiet python -c "import xdist" 2>/dev/null && ok "pytest-xdist installed (the gate runs unit tests in parallel)" || fail "pytest-xdist missing (the gate's unit step needs it)" "uv sync"
fi
if [ -f .venv/bin/pytest ]; then
  shebang="$(head -1 .venv/bin/pytest | sed 's/^#!//')"
  # Compare physical paths: /var and /private/var (or any symlinked folder) are the same place.
  bin_dir="$(cd "$(dirname "$shebang")" 2>/dev/null && pwd -P || true)"
  if [ "$bin_dir" = "$(cd .venv/bin && pwd -P)" ]; then ok "venv scripts point at this repo"; else fail "venv scripts point at another folder ($shebang): the repo moved" "uv sync --reinstall"; fi
fi
if [ -x .venv/bin/python ]; then
  hw="$(if [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then echo arm64; else uname -m; fi)"
  va="$(.venv/bin/python -c 'import platform; print(platform.machine())' 2>/dev/null)"
  [ "$va" = "$hw" ] && ok "venv architecture $va matches hardware" || fail "venv is ${va:-broken} on $hw hardware" "rm -rf .venv && uv venv --python cpython-3.12-macos-aarch64-none && uv sync"
fi
role="$(uv run --quiet careerbrew machine show 2>/dev/null | sed -nE 's/^role: *([a-z]+).*/\1/p')"  # machine.yaml is encrypted at rest
ok "machine role: ${role:-personal (default)}"
key="${CAREERBREW_KEY_FILE:-$HOME/.config/careerbrew/backup.key}"
if [ -f "$key" ]; then
  [ "$(stat -c %a "$key" 2>/dev/null || stat -f %Lp "$key")" = 600 ] && ok "backup key is mode 600" || fail "backup key permissions too open" "chmod 600 $key"
else
  ok "backup key not created yet (first \`careerbrew backup create\` makes it)"
fi
# Which notifier shows pop-ups (issue #74). Optional: every notification is also in the web app.
pop="$(uv run --quiet careerbrew notify status 2>/dev/null | sed -n 's/^pop-ups: //p')"
ok "notifications: ${pop:-unknown (run: uv run careerbrew notify status)}"
if curl -s -m 2 http://localhost:11434/api/tags >/dev/null 2>&1; then ok "local model (ollama) reachable"; else ok "local model not running: LLM steps use deterministic fallbacks (optional: ollama serve)"; fi
# A deployed /careerbrew-* command older than its source makes an agent follow old steps (task checkup-command-drift).
# The venv's python has PyYAML for agentbrew's state file; a crash of the check is a failure, never a pass.
py=python3
[ -x .venv/bin/python ] && py=.venv/bin/python
if command -v "$py" >/dev/null 2>&1; then
  out="$("$py" scripts/check-deployed-commands.py 2>&1)"
  code=$?
  if [ "$code" -le 1 ] && ! grep -q Traceback <<< "$out" && grep -qE '^(ok|FAIL) ' <<< "$out" && { [ "$code" = 0 ] || grep -q '^FAIL ' <<< "$out"; }; then
    while IFS= read -r line; do
      echo "$line"
      case "$line" in FAIL*) fails=$((fails + 1)) ;; esac
    done <<< "$out"
  else
    fail "deployed commands check crashed (exit $code)" "$py scripts/check-deployed-commands.py"
  fi
fi
if git rev-parse --git-dir >/dev/null 2>&1; then
  [ -z "$(git ls-files private)" ] && ok "no private/ file tracked by git" || fail "private/ files are tracked" "git rm -r --cached private"
fi

[ "$fails" -eq 0 ] && echo "doctor: all checks passed" || echo "doctor: $fails check(s) failed"
[ "$fails" -eq 0 ]
