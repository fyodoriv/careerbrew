#!/usr/bin/env bash
# Move a careerbrew checkout from the old private history to the public history.
#
# careerbrew moved to a new history that starts at one commit (2026-11-12). A checkout
# made before that day cannot fast-forward: `git status` says "diverged" and the
# two histories share no commit. This script moves such a checkout over.
#
# It never deletes work:
#   - every local branch, tag and stash goes into one bundle file first;
#   - old branches stay in the clone under refs/pre-public/, where a plain
#     `git push` does not send them;
#   - `private/` and every other ignored file stays where it is;
#   - it stops, and changes nothing, when a tracked file has uncommitted changes.
#
# Run it from the checkout. It is safe to run again.
#   git fetch origin main && git show origin/main:scripts/migrate-history.sh | bash
#
# Exit 0: the checkout is on the public history. Exit 1: stopped, nothing lost;
# the last line says what to do. Exit 2: this is not a careerbrew checkout.
set -euo pipefail

# The first commit of the public history. A test may set another one.
PUBLIC_ROOT="${CAREERBREW_PUBLIC_ROOT:-22e3e591b70dd8a5f8a1111355201b85c8f68ed5}"
SAVE_DIR="${CAREERBREW_MIGRATE_SAVE_DIR:-$HOME/.local/state/careerbrew-cutover}"

say() { printf 'migrate-history: %s\n' "$*"; }
stop() { say "stopped: $*"; exit 1; }

top="$(git rev-parse --show-toplevel 2>/dev/null)" || { say "not a git checkout"; exit 2; }
cd "$top"
[ -f Agentfile.yaml ] && [ -d src/careerbrew ] || { say "not a careerbrew checkout: $top"; exit 2; }
git remote get-url origin >/dev/null 2>&1 || { say "the checkout has no remote named origin"; exit 2; }

git fetch --quiet --no-tags origin main || stop "cannot fetch origin main. Sign in to GitHub (gh auth login) and check the network, then run this again."
new="$(git rev-parse --verify --quiet origin/main)" || stop "origin has no main branch"
roots="$(git rev-list --max-parents=0 origin/main)"
[ "$roots" = "$PUBLIC_ROOT" ] || stop "origin/main does not start at the public first commit. Check that origin is the public careerbrew repository: $(git remote get-url origin)"

if [ "$(git rev-list --max-parents=0 HEAD 2>/dev/null)" = "$PUBLIC_ROOT" ]; then
  say "this checkout is already on the public history. Nothing to do."
  exit 0
fi

# Nothing below may lose work. Tracked changes must be committed first.
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  stop "tracked files have uncommitted changes. Commit them on a branch (they go into the bundle), then run this again."
fi
branch="$(git symbolic-ref --quiet --short HEAD || true)"
[ -n "$branch" ] || stop "HEAD is detached. Check out a branch, then run this again."
others="$(git worktree list --porcelain | grep -c '^worktree ' || true)"
[ "$others" -le 1 ] || stop "$((others - 1)) linked worktree(s) exist. Finish or remove each one (git worktree list; git worktree remove <path>), then run this again."

# 1. Save every ref, stashes included, outside the checkout.
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$SAVE_DIR"
bundle="$SAVE_DIR/pre-public-$stamp.bundle"
git bundle create "$bundle" --all >/dev/null 2>&1 || stop "could not write the bundle to $SAVE_DIR"
git bundle verify "$bundle" >/dev/null 2>&1 || stop "the bundle did not verify: $bundle"
say "saved every branch, tag and stash to $bundle"

# 2. Move each old branch out of refs/heads/, so a plain push cannot send old history.
moved=0
while read -r sha ref; do
  name="${ref#refs/heads/}"
  [ "$(git rev-list --max-parents=0 "$sha" | head -1)" = "$PUBLIC_ROOT" ] && continue
  git update-ref "refs/pre-public/$name" "$sha"
  [ "$name" = "$branch" ] || git update-ref -d "$ref"
  moved=$((moved + 1))
done < <(git for-each-ref --format='%(objectname) %(refname)' refs/heads/)

# 3. Put the working tree on the public main. Ignored files stay.
git checkout --quiet -B main origin/main || stop "git could not check out the public main. An untracked file may be in the way: move it, then run this again. Your old branch is at refs/pre-public/$branch and in the bundle."
git branch --quiet --set-upstream-to=origin/main main
[ "$branch" = main ] || git update-ref -d "refs/heads/$branch"
git remote prune origin >/dev/null 2>&1 || true

say "moved $moved old branch(es) to refs/pre-public/. Never push them: they hold the private history."
say "the checkout is on the public history at $(git rev-parse --short HEAD)."
say "next: scripts/setup.sh, then scripts/doctor.sh"
