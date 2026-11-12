#!/usr/bin/env bash
# One-line setup: install everything, then prove the machine is ready.
# Safe to run again; each step skips work that is already done.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

if [ -f Cargo.toml ]; then
  source scripts/rust-readiness.sh
  rust_readiness
fi

step "Install dependencies"
if [ "${SETUP_SKIP_DEPS:-0}" = 1 ]; then
  echo "skipped (SETUP_SKIP_DEPS=1)"
else
  if [ -f package.json ]; then
    if [ -f pnpm-lock.yaml ] || grep -q '"packageManager": *"pnpm' package.json; then
      corepack enable >/dev/null 2>&1 || true
      pnpm install --frozen-lockfile 2>/dev/null || pnpm install
    else
      npm ci 2>/dev/null || npm install
    fi
  fi
  if [ -f pyproject.toml ]; then
    uv sync
    # After the repo moves, console scripts keep absolute shebangs to the old folder; uv sync does not rewrite them.
    if [ -f .venv/bin/pytest ]; then
      shebang_dir="$(cd "$(dirname "$(head -1 .venv/bin/pytest | sed 's/^#!//')")" 2>/dev/null && pwd -P || true)"
      [ "$shebang_dir" = "$(cd .venv/bin && pwd -P)" ] || { echo "venv scripts point at an old folder: reinstalling"; uv sync --reinstall; }
    fi
  fi
  if [ -f Cargo.toml ]; then PATH="$RUST_READY_BIN:$PATH" RUSTUP_AUTO_INSTALL=0 "$RUST_READY_BIN/cargo" fetch; fi
fi

step "Web app (TypeScript)"
if [ -f web/package.json ] && command -v npm >/dev/null 2>&1; then
  if [ "${SETUP_SKIP_DEPS:-0}" = 1 ]; then echo "skipped (SETUP_SKIP_DEPS=1)"; else (cd web && npm ci --no-audit --no-fund && npm run build); fi
else
  echo "note: Node.js missing, so the web app is not built (install Node 20+, then rerun); the CLI works without it"
fi

step "Create local folders (never committed)"
mkdir -p .metrics
mkdir -p private
chmod 700 private
[ -f .env ] || { [ -f .env.example ] && cp .env.example .env && echo "created .env from .env.example"; } || true

step "Install git hooks"
if [ -f lefthook.yml ] && command -v lefthook >/dev/null 2>&1; then
  if hooks_path="$(git config --get core.hooksPath)"; then
    printf 'git hooks: existing path %s retained; repository entry: hooks/pre-commit (delegation not verified)\n' "$hooks_path"
  else
    hook_lookup_status=$?
    if [ "$hook_lookup_status" -eq 1 ]; then
      lefthook install >/dev/null 2>&1 || echo "note: lefthook install failed (fix: check its configuration)"
    else
      echo "git hooks: cannot read the configured hook path (fix: repair Git configuration and rerun setup)" >&2
      exit 1
    fi
  fi
fi

step "Install the careerbrew command"
scripts/install-launcher.sh

step "Health check"
scripts/doctor.sh

printf '\nReady. Next: scripts/check.sh\n'
