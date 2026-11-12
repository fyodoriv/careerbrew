#!/usr/bin/env bash
# Public-readiness checks for the open-source release. The release plan is in the project
# journal, which is not part of the public tree.
# Prints one line per exposure surface, S1 to S22: id, kind, status, count, label. Then one summary line.
# It reads the tracked content of a git ref, never the working tree. It never prints matched text.
# Exit 0 only when no surface is fail or error. With --release, an unmeasured, unsigned or
# owner-only surface blocks too. Exit 2 on a usage error.
# Every check reads the release tree: the ref without the paths in config/public-release-exclude.txt.
# That list must work: a missing or empty list, a line that matches no file, or a list that leaves
# out no file or every file makes each surface that reads the tree `error`.
# Usage: scripts/public-readiness.sh [--release] [--released] [--first-run] [--allow-home-override] [--tree <git-ref>]
#          [--tree-hash | --release-tree-id | --write-release-tree <dir> | --list-dangling | --list-binaries]
#   --release    owner mode (the owner's Mac only): also S1, S2, S14, S16, S20 and the review signatures.
#                S1 reads its pattern from the file that OSS_READINESS_ENV names
#                (default ~/.config/oss-readiness/oss-readiness.env).
#   --tree-hash  print the hash that a review signature must carry, then stop.
#   --release-tree-id            print the id of the git tree object of the release tree, then stop.
#   --write-release-tree <dir>   write the release tree into a new or empty folder, then stop.
#   --list-dangling              print the S21 files, one `<paths>` TAB `<file>` line each, then stop.
#   --list-binaries              print `<path>` TAB `<sha256>` per non-text file: the rows that
#                docs/proof/public-readiness-binaries.tsv must hold (surface S7).
#   --first-run  S17 also runs setup, doctor and the fast gate in a scratch copy of the release
#                tree with a scratch HOME and scratch XDG folders. Slow. Without it S17 runs
#                scripts/check-docs.sh there and can fail, but cannot pass.
#   --allow-home-override   owner mode: CAREERBREW_HOME is set on purpose. Without this flag S16 and
#                S20 are `unmeasured` when CAREERBREW_HOME is set, because they would read that data folder.
#   --released   the ref is the release commit itself: it must hold the list and no file that the
#                list leaves out. Use it for every run on the release commit.
# PUBLIC_READINESS_ACCOUNT names a macOS account for S3 to look for (owner mode: the current user).
# PUBLIC_READINESS_ZONE names the owner's IANA time zone for S20 (default: the `timezone` setting,
# then the system zone). PUBLIC_READINESS_PLACE_TERMS adds words of the owner's place for S20, as
# a comma list (a city, a region). It is read in owner mode only and never printed.
# PUBLIC_READINESS_STEP_SECONDS sets the time limit of one S17 step. S10 reads licences from installed Python packages and from CARGO_HOME.
# S14 finds the gh text guard and the public push list next to the `gh` on PATH
# (DOTFILES_PUBLIC_PUSH_ALLOWLIST names another list).
# Known limits: see the top of scripts/public-readiness.py. The pattern file's minimum version
# is not checked here; a missing file or an empty pattern fails S1.
set -euo pipefail
set +x # never trace the private pattern
cd "$(dirname "$0")/.."

release=0
for arg in "$@"; do [ "$arg" = --release ] && release=1; done

py=.venv/bin/python
[ -x "$py" ] || py=python3

# A pattern from the caller's shell never stands in for the file, and no child inherits one.
unset OSS_READINESS_INTERNAL_PATTERN PUBLIC_READINESS_PATTERN PUBLIC_READINESS_PATTERN_FILE PUBLIC_READINESS_PATTERN_FD
[ "$release" = 1 ] || exec "$py" scripts/public-readiness.py "$@"

env_file="${OSS_READINESS_ENV:-$HOME/.config/oss-readiness/oss-readiness.env}"
pattern=""
found=""
if [ -r "$env_file" ]; then
  found=1
  # A subshell loads the file, so nothing else from it reaches this script.
  pattern="$(
    set +eu
    # shellcheck disable=SC1090
    . "$env_file" >/dev/null 2>&1
    printf '%s' "${OSS_READINESS_INTERNAL_PATTERN:-}"
  )" || pattern=""
fi
# The pattern goes to this one process through a pipe on descriptor 3. It is never in an
# environment or an argument list, and no child of that process inherits the pipe.
PUBLIC_READINESS_PATTERN_FD=3 PUBLIC_READINESS_PATTERN_FILE="$found" exec "$py" scripts/public-readiness.py "$@" 3< <(printf '%s' "$pattern")
