# Grind gate batching

### Batch the full gate (up to 4 ready PRs)

The full gate's e2e suite takes about 6 minutes, and every gate queues on the one `heavy-run` lock. When two or more PRs are ready, run one full gate for the batch instead of one per PR. The owner approved this only without loss of stability, so keep every rule:

1. Each PR first passes `scripts/check.sh fast` and its own e2e stories (the `tests/e2e/us-*.sh` files it changes) alone. The script reruns both for each PR before the batch.
2. Run `scripts/batch-gate.sh <pr>... > <log> 2>&1; rc=$?` outside the sandbox (it calls `gh`), and wait for it in the foreground like a gate. It merges `origin/main` and every PR head into a scratch worktree (`../wt.noindex/cb-batch-scratch`, reused so it stays warm) and runs one full `scripts/check.sh` there through `heavy-run`. Its last line is one summary; its logs are in `.metrics/batch-gate/`.
3. Read the exit code:
   - `0`: the gate passed, and it squash-merged the PRs in the order given. After each merge, `main`'s tree equals the tree it gated. A PR that conflicts with `main` plus the PRs before it is left out before any check runs; the summary names it (`not merged: #N conflicts ...`): merge `main` into it, resolve, push, and put it in the next batch.
   - `1`: nothing from the batch merged. It bisects by halves: a PR named "fails alone" goes back to its worker; PRs that "fail only together" ship one at a time.
   - `2`: refused. A batch holds at most 4 PRs, and never a draft, a PR that does not target `main`, or a PR that touches a gate file (`scripts/check.sh`, `scripts/input-key.sh`, `scripts/e2e.sh`, `tests/e2e/lib.sh`, or `web/playwright*.config.ts`). Ship those alone with their own full gate.
   - `3`: stopped. Either `main` moved after the gate (nothing merged: run the batch again), or a merge failed or left `main` on a tree it did not gate. Then it reran the full gate on `main` and merged nothing more. If that gate failed, fix `main` first; put the rest in a new batch.
4. The batch's full gate counts as each PR's full gate. A PR body's Proof names the batch (`gate: batch-gate #<a> #<b> ...`) with the summary line, and the batch report lists it.

Never `--admin`, never force-push, never rebase a pushed branch to make a batch fit.

You may ship several items at once, each in its own worktree. Give each one its own `CAREERBREW_E2E_PORT` (for example 8891, 8892) before any Playwright run or `scripts/check.sh`, so the suites never share port 8799. Merge one PR at a time. Before its first push, rebase the next one on the new `origin/main`; after a push, merge `origin/main` in instead. Rerun its gate before you merge it.
