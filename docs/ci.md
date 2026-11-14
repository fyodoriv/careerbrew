# Hosted checks (CI)

Every pull request and every push to `main` runs the gate on clean GitHub
runners ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)).
**The hosted checks are the merge gate.** A pull request merges when
they are green. Nobody waits for a full gate on their own machine.

## What runs

| Check | Runner | Commands |
|---|---|---|
| `gate (ubuntu-24.04)` | `ubuntu-24.04` | `scripts/setup.sh`, then `scripts/check.sh` |
| `gate (macos-15)` | `macos-15` | the same |
| `public-readiness` | `ubuntu-24.04` | `scripts/public-readiness.sh --released` ([workflow](../.github/workflows/public-readiness.yml)): fails on a path the public tree leaves out, a secret-shaped value, or a link to a left-out path; prints counts only |

Each job installs the tools a new machine lacks: uv, Node 24, the Rust version
that `rust-toolchain.toml` pins, gitleaks (a pinned release with a checked
SHA-256), and Playwright's Chromium. Then it runs the same two commands a
contributor runs.

The uv step pins the exact Python version, because the frozen HTTP reference
of the Rust capture tests holds it in a `Server` header
(`tests/unit/test_hosted_ci.py` keeps the two equal). One step links the uv
cache to its default path: a capture test runs uv with a cleared environment.

Each job also makes a local `main` branch (a pull request checkout is detached,
and one story bundles `main`), puts the pinned Rust tools first on `PATH` (not
the rustup proxies), and sets an empty telemetry key, so a runner sends no
usage data.

Every Playwright config sets `captureGitInfo: { commit: false, diff: false }`.
Without it, in a pull request on GitHub Actions, Playwright runs
`git fetch origin <base> --depth=1`. That makes the checkout shallow, and the
corporate rehearsal (US-40) then bundles a `main` that has no parents and
cannot be cloned.

The runners set `UNIT_JOBS=2` and `E2E_JOBS=2`: a standard runner has few CPUs.

## Work with the hosted checks

1. While you work, run the tests you changed, or `scripts/check.sh fast`.
2. Push when the change is reviewable. The pre-push hook takes seconds: it
   checks the push context, runs the leak scan, and runs gitleaks. It runs no test.
3. Open the pull request. Wait with `gh run watch <run-id> --interval 30` in
   the background and do other work.
4. Merge when `gh pr checks <pr>` shows all three checks green.

The lesson of a change goes in the pull request body (Predicted / Observed /
Lesson). Do not add a file to the learnings folder under `docs/`: the public
tree leaves that folder out, so `public-readiness` fails that pull request.

A full local gate (`scripts/check.sh`) still works and is optional. Use it
when the hosted checks are unavailable.

## Safety rules

Keep every rule when you change a workflow.

- Use only the standard runners `ubuntu-24.04` and `macos-15`. Never a larger runner.
- Use no repository secret.
- Never use the `pull_request_target` trigger. A fork pull request must get a read-only token.
- Keep `permissions: contents: read`.
- Upload no artifact. A log or a trace would be a public download.
- Never copy `private/`, a real `.env`, or the owner's private-pattern file to a runner.
- Pin every action to a full 40-character commit SHA, with the version in a trailing comment.
- Install a tool from a release only with a pinned version and a checked SHA-256. Never `curl | sh`.

## What the hosted gate cannot check

Two checks stay local.

**Approved pictures (US-75).** The pictures in `web/visual/__screenshots__` are
drawn on a developer Mac and compared with no pixel of slack. A hosted macOS
runner draws each screen 50 to 160 pixels off (run 43493131555: 176 of 196
visual tests failed that way). So the workflow sets
`CAREERBREW_VISUAL_PICTURES=local-only`, and the story then checks that every
story has its pictures and that Playwright lists every visual test, but
compares none. The Linux job skips the story: no Linux pictures exist. After a change to a screen, run `cd web && npm run build &&
npm run visual` on your Mac. Task `hosted-visual-pictures` tracks pictures a
runner can check.

**The private pattern.** The owner's private-pattern file lives only on the owner's machine. So the
private-pattern leak check runs only in the local git hooks. The hosted gate
still runs the repo's own privacy scan (`scripts/leak-check.sh`) and gitleaks.

## Read a failed run

1. Find the run: `gh pr checks <pr>` or `gh run list --workflow ci.yml --limit 5`.
2. Read the failing step first: `gh run view <run-id> --log-failed`.
3. Reproduce the step on your machine before you change code.
4. A macOS job that was cancelled at its time limit with no log got no runner.
   Read `gh api repos/{owner}/{repo}/check-runs/<job-id>/annotations`, then
   `gh run rerun <run-id> --failed`.

Wait for a run with `gh run watch <run-id> --interval 30`.

## Cost rule

This project never spends money. Standard runners are free for a public
repository. If the repository becomes private, turn the workflow off first:
a macOS minute on a private repository is billed as 10.

## Measure it

- Result time of the last runs: `gh run list --workflow ci.yml --limit 10 --json conclusion,createdAt,updatedAt`. Target: under 45 minutes.
- Open-to-merge time: `gh pr list --state merged --limit 10 --json number,createdAt,mergedAt`.
- Runner labels of one run: `gh run view <run-id> --json jobs -q '.jobs[].labels'`. Only `ubuntu-24.04` and `macos-15` may appear.
- Owner step, once after the first week: read the billing page and confirm 0 paid minutes. An agent cannot see that page.

## Branch rule on `main`

`main` requires the three checks. The owner approved this rule on 2026-11-13.
This call sets it (an administrator of the repository runs it):

```bash
gh api -X PUT repos/fyodoriv/careerbrew/branches/main/protection --input - <<'JSON'
{
  "required_status_checks": {
    "strict": false,
    "checks": [
      {"context": "gate (ubuntu-24.04)"},
      {"context": "gate (macos-15)"},
      {"context": "public-readiness"}
    ]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
JSON
```

- Read it: `gh api repos/fyodoriv/careerbrew/branches/main/protection --jq '.required_status_checks.checks[].context'`.
- Remove it: `gh api -X DELETE repos/fyodoriv/careerbrew/branches/main/protection`.
- `enforce_admins` is `false`, so the owner can still merge when GitHub itself
  is down. Agents never use that path: a red check is read and fixed.
- If a check name changes in a workflow, change it here in the same pull request.

## Action updates

`.github/dependabot.yml` opens a weekly pull request when a pinned action has
a newer release. Review the release notes, then merge on green checks.

## Change an action version by hand

Get the commit of a release tag, then replace the SHA and the comment:

```bash
gh api repos/<owner>/<action>/commits/<tag> --jq .sha
```
