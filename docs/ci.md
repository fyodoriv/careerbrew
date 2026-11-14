# Hosted checks (CI)

Every pull request and every push to `main` runs the gate on clean GitHub
runners. The workflow is [`.github/workflows/ci.yml`](../.github/workflows/ci.yml).

## What runs

| Check | Runner | Commands |
|---|---|---|
| `gate (ubuntu-24.04)` | `ubuntu-24.04` | `scripts/setup.sh`, then `scripts/check.sh` |
| `gate (macos-15)` | `macos-15` | the same |

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
compares none. After a change to a screen, run `cd web && npm run build &&
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

## Change an action version

Get the commit of a release tag, then replace the SHA and the comment:

```bash
gh api repos/<owner>/<action>/commits/<tag> --jq .sha
```
