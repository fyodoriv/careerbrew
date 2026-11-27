#!/usr/bin/env bash
# US-72 receipt check: each discovered browser test passed once.
#   scripts/browser-test-receipts.sh <discovery.json> <execution.json>
# Runs `careerbrew-xtask browser-test-receipts` when the built binary is current,
# else scripts/browser-test-receipts.py; both print the same lines and exit codes.
# CAREERBREW_BROWSER_RECEIPT passes through to either.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
# Relative argument paths keep the caller's folder: pick the tool in a subshell.
xtask_bin="$(
  cd "$here"
  # Native dispatch (rust-27b): run the built careerbrew-xtask when it is newer
  # than every file it is built from; otherwise the shell code below runs.
  # CAREERBREW_NATIVE_TOOLS=0 forces the shell code; CAREERBREW_XTASK_BIN names a
  # binary to use as is (parity tests). Parity: xtask/tests/native_dispatch.rs.
  xtask_bin="${CAREERBREW_XTASK_BIN:-}"
  if [ "${CAREERBREW_NATIVE_TOOLS:-1}" = 0 ]; then
    xtask_bin=""
  elif [ -z "$xtask_bin" ] && [ -x target/debug/careerbrew-xtask ] && [ -f xtask/Cargo.toml ]; then
    xtask_bin="$PWD/target/debug/careerbrew-xtask"
    newer="$(find xtask/src xtask/Cargo.toml Cargo.toml Cargo.lock rust-toolchain.toml crates/vault crates/setup crates/policy -newer "$xtask_bin" 2>/dev/null | head -n 1)" || true
    [ -z "$newer" ] || xtask_bin=""
  fi
  printf '%s' "$xtask_bin"
)"
if [ -n "$xtask_bin" ]; then exec "$xtask_bin" browser-test-receipts "$@"; fi
exec "$here/.venv/bin/python" "$here/scripts/browser-test-receipts.py" "$@"
