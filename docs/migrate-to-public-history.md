# Move an old checkout to the public history

careerbrew moved to a new history on 2026-11-12, to get ready for open source.
The repository now starts at one new commit. A checkout made before that day holds the old private history, and
git cannot fast-forward it.

## How to tell

Any one of these means the checkout is old:

- `careerbrew day start --update-only` reports the careerbrew checkout as `diverged`.
- `git pull --ff-only` says the branches have diverged, or "refusing to merge unrelated histories".
- `git fetch origin main && git merge-base HEAD origin/main` prints nothing.

## Agents: what to do

Stop the work that needs careerbrew. Tell the person in one line that careerbrew
moved to a public history and that you will move this checkout. Then run the
steps below. Do not `git pull`, `git reset`, `git merge` or push from the old
checkout.

## Steps

Run them in the checkout (`careerbrew where` prints its folder).

1. Get the script from the public history and run it:

   ```bash
   git fetch origin main && git show origin/main:scripts/migrate-history.sh > "${TMPDIR:-/tmp}/migrate-history.sh" && bash "${TMPDIR:-/tmp}/migrate-history.sh"
   ```

2. If it stops, do what its last line says, then run it again. It stops when:
   - a tracked file has uncommitted changes: commit them on a branch first;
   - a file is marked assume-unchanged or skip-worktree: clear the mark first;
   - a file on disk is in the way of a file the new history tracks: move it out first;
   - `HEAD` is detached: check out a branch first;
   - a linked worktree exists: finish or remove it first (`git worktree prune` for a folder that is gone);
   - `origin` is not the public careerbrew repository.

3. Set up and check the checkout:

   ```bash
   scripts/setup.sh
   ```

   ```bash
   scripts/doctor.sh
   ```

4. On a corporate machine, also check the company plugin:

   ```bash
   uv run careerbrew machine ready
   ```

   The summary line must say `corporate readiness: N/N`, with the same N as before the move.

## What the script does

- It saves every branch, tag and stash in one bundle file under `~/.local/state/careerbrew-cutover/`.
- It moves each old branch to `refs/pre-public/<name>` and each old tag to `refs/pre-public-tags/<name>` in the same clone. `git push` and `git push --tags` do not send those. A branch that merged both histories counts as old.
- It keeps your stashes, and a copy of each one under `refs/pre-public-stash/`.
- It puts `main` on the public history and sets its upstream. A local `main` that is already on the public history stays as it is.
- A second run finishes a move that stopped half way.
- It leaves `private/`, the plugin folders and every other ignored file as they are. Your data and settings do not move.

## What stays the same

- The data folder, its encryption key, and all settings.
- Company plugins: a plugin lives in its own repository and its interface did not change.
- Commands: `careerbrew`, `/careerbrew`, `/work`.

## What changed

- Issue and pull request numbers restart. Old numbers point to nothing in the public repository.
- The work journals (`docs/plans/`, `docs/learnings/`) are not in the public tree. The bundle holds your old copy.
- Old branches hold the private history. Never push one to the public repository. To continue old work, make a new branch from `main` and apply the change there:

  ```bash
  git diff refs/pre-public/main...refs/pre-public/<branch> | git apply --3way
  ```
