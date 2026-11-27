#!/usr/bin/env bash
# Print the repo's input hash: 16 hex characters over the content of the tracked
# and untracked files that git does not ignore, like the nx cache.
#   scripts/input-key.sh [pathspec...] hashes only the files that match the git
#   pathspecs, like an nx target's inputs (e.g. ':(exclude)TASKS.md'). With no
#   pathspec it hashes every file.
#   scripts/check.sh keys its step cache by it (.metrics/cache/<step>).
#   scripts/e2e.sh records its per-file results under it (.metrics/e2e-results.json),
#   for consumers that display the last recorded run.
# Outside a git checkout, or on any git error, it exits 1: then nothing is cached.
set -euo pipefail
cd "$(dirname "$0")/.."
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
# A leading `--` is a pathspec line here but a separator natively: keep the shell code.
if [ -n "$xtask_bin" ] && [ "${1:-}" != -- ]; then
  if [ $# -eq 0 ]; then exec "$xtask_bin" input-key --repo "$PWD"; fi
  exec "$xtask_bin" input-key --repo "$PWD" -- "$@"
fi
git rev-parse --git-dir >/dev/null 2>&1 || exit 1
# A git hook exports GIT_DIR and friends; hash this checkout, not the hook's index.
unset $(git rev-parse --local-env-vars)

# The key hashes file content only, so it is the same in fast and full mode and
# before and after `git add` or `git commit`: a later run reuses a
# green gate on the same files. A copy of the index records the working tree;
# --info-only writes no objects.
# cp -p keeps the index mtime: git re-reads a file as new as the index (racy
# git), so a same-size edit in the second of a `git add` is not missed.
tmp_index="$(mktemp "${TMPDIR:-/tmp}/check-index.XXXXXX")"
trap 'rm -f "$tmp_index"' EXIT
cp -p "$(git rev-parse --git-path index)" "$tmp_index" 2>/dev/null || rm -f "$tmp_index"
export GIT_INDEX_FILE="$tmp_index"
git ls-files -z --modified --others --exclude-standard |
  git update-index -z --add --remove --info-only --stdin
if [ $# -eq 0 ]; then
  git ls-files -s | shasum | cut -c1-16
else
  # The pathspecs are part of the key: a step whose inputs change misses the cache.
  { printf '%s\n' "$@"; git ls-files -s -- "$@"; } | shasum | cut -c1-16
fi
