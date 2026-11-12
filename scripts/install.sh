#!/usr/bin/env bash
# Install careerbrew for one person on one machine (personal or corporate).
#
#   scripts/install.sh [--role personal|corporate] [--plugin <git-url-or-folder>]... [--label <name>]
#
# Run it from a clone of the repo. It is idempotent: rerun it to add a plugin or change the role.
# On a terminal with no --role it asks for the role (Enter keeps the current one); with no terminal
# (a setup chain) it never waits. Exit 0 means installed; 1 a missing tool or failed step; 2 a bad option.
# An agent follows INSTALL.md: it explains the plan and asks the questions first.
# Each person keeps their own data root ($CAREERBREW_HOME, default <repo>/private); nothing is shared
# between people except the plugins they choose to install (for example one company plugin plus
# their own team plugin, which `extends` it).
set -euo pipefail
cd "$(dirname "$0")/.."

role="" label="" plugins=()
while [ $# -gt 0 ]; do
  case "$1" in
    --role) role="${2:?--role needs personal or corporate}"; shift 2 ;;
    --plugin) plugins+=("${2:?--plugin needs a git URL or folder}"); shift 2 ;;
    --label) label="${2:?}"; shift 2 ;;
    -h|--help) sed -n 2,12p "$0"; exit 0 ;;
    *) echo "unknown option $1 (see --help)" >&2; exit 2 ;;
  esac
done

command -v git >/dev/null || { echo "git is missing: install Xcode command line tools (xcode-select --install)" >&2; exit 1; }
command -v uv >/dev/null || { echo "uv is missing: curl -LsSf https://astral.sh/uv/install.sh | sh" >&2; exit 1; }

# Native CPU arch; on Apple Silicon sysctl sees through Rosetta (uname -m may say x86_64 there).
# HW_ARCH and VENV_PYTHON override the probes (tests use them).
hw_arch() { if [ -n "${HW_ARCH:-}" ]; then echo "$HW_ARCH"; elif [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then echo arm64; else uname -m; fi; }
venv_arch() { "${VENV_PYTHON:-.venv/bin/python}" -c 'import platform; print(platform.machine())' 2>/dev/null; }
if [ "${SETUP_SKIP_DEPS:-0}" != 1 ]; then uv sync --quiet; fi
if [ "$(hw_arch)" = arm64 ] && [ -n "$(venv_arch)" ] && [ "$(venv_arch)" != arm64 ]; then
  echo "the .venv is $(venv_arch) on arm64 hardware (an x86_64 Python was picked)" >&2
  if [ "${SETUP_SKIP_DEPS:-0}" = 1 ]; then
    echo "fix: rm -rf .venv && uv venv --python cpython-3.12-macos-aarch64-none && uv sync" >&2; exit 1
  fi
  rm -rf .venv
  uv venv --quiet --python cpython-3.12-macos-aarch64-none && uv sync --quiet
  [ "$(venv_arch)" = arm64 ] || { echo "could not build an arm64 .venv (fix: uv python install cpython-3.12-macos-aarch64-none, then rerun)" >&2; exit 1; }
fi
cb() { if [ -n "${CAREERBREW_BIN:-}" ]; then "$CAREERBREW_BIN" "$@"; elif [ -x .venv/bin/careerbrew ]; then .venv/bin/careerbrew "$@"; else uv run --quiet careerbrew "$@"; fi; }

# The web app (careerbrew serve) needs web/dist; build it when node is here.
if [ "${SETUP_SKIP_DEPS:-0}" != 1 ] && [ ! -f web/dist/index.html ]; then
  if command -v npm >/dev/null; then
    (cd web && npm ci --no-audit --no-fund --silent && npm run build --silent) >/dev/null || { echo "web app build failed (fix: cd web && npm ci && npm run build)" >&2; exit 1; }
  else
    echo "node is missing, so the web app is not built (fix: brew install node, then cd web && npm ci && npm run build)" >&2
  fi
fi
# Issue #175: ask for the role on a terminal instead of keeping one silently; a setup chain never waits.
if [ -z "$role" ] && [ -t 0 ] && [ -t 1 ]; then
  current="$(cb machine show 2>/dev/null | sed -n 's/^role: \([a-z]*\).*/\1/p' || true)"  # a CLI that fails here fails loudly at init
  current="${current:-personal}"
  printf 'Which machine is this? personal (job hunt and your own work) or corporate (a work laptop: job-hunt commands are blocked) [%s]: ' "$current"
  read -r answer || answer=""
  role="$(printf %s "${answer:-$current}" | tr '[:upper:]' '[:lower:]')"
  case "$role" in personal|corporate) ;; *) echo "unknown role $role: use personal or corporate (or pass --role)" >&2; exit 2 ;; esac
fi
# Role first, so a corporate install never writes job-hunt files, not even briefly.
if [ -n "$role" ]; then cb machine set "$role" ${label:+--label "$label"}; fi
cb init
# Company plugin first, then team plugins that extend it: install in the order given.
for p in "${plugins[@]+"${plugins[@]}"}"; do cb plugin add "$p"; done
cb plugin list
cb machine show
# Last: prove the CLI imports (a wrong-arch native wheel fails here, not later).
cb --version >/dev/null || { echo "careerbrew does not start (fix: rm -rf .venv && scripts/install.sh)" >&2; exit 1; }
if [ "${role:-$(cb machine show 2>/dev/null | sed -n 's/^role: \([a-z]*\).*/\1/p' || true)}" = corporate ]; then
  echo "careerbrew installed (corporate). Next: follow docs/corporate-machine.md, then careerbrew machine ready."
else
  echo "careerbrew installed. Next: careerbrew connect <tool> (or careerbrew secret set ...), then careerbrew serve to open the web app."
fi
# Issue #175: the feedback step, as in agentbrew and dotfiles (CONTRIBUTING.md, "Report problems and ideas").
echo "Something wrong or confusing? Report it: agentbrew report --repo fyodoriv/careerbrew (it scrubs private details and shows the text first). Fixed it yourself? agentbrew contribute opens the PR."
