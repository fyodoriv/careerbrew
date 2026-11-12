---
description: Run the careerbrew repo checkup (open PRs first, health, gate, broken stories, speed, P0s), show its run page, ask which next action to take, and file each problem found
allowed-tools: Bash(scripts/checkup.sh:*), Bash(NO_CACHE=1 scripts/checkup.sh:*), Bash(uv run careerbrew report:*), Bash(uv run careerbrew config), Bash(uv run careerbrew config set checkup_artifact_url:*), Bash(uv run careerbrew open:*), Bash(uv run careerbrew privacy scan:*), Bash(gh issue list:*), Bash(gh pr list:*), Bash(gh repo view:*)
---

From the repo root: if `$ARGUMENTS` contains `full`, run `NO_CACHE=1 scripts/checkup.sh`; otherwise run `scripts/checkup.sh`.

Say its opening line first: last checkup here, result, and commits per repo since then. Repo facts are recorded at `${XDG_STATE_HOME:-~/.local/state}/checkups/careerbrew.json` when done.

Output is a run-page path and one summary line (`repo-checkup: ok|blocked: N done, N blocker(s), N next action(s); next: <command>`). Exit code is the overall result.

Then:

1. Read the whole `uv run careerbrew report latest --run repo-checkup --json` report: counts, ids, statuses and exact Next commands. Do not grep it.
2. Show the page. You may publish the page file as an Artifact if the tool is available and JSON says `"personal": false` (repo facts only); otherwise run `uv run careerbrew open --report repo-checkup`. Never publish `"personal": true`.
3. With Artifact, refresh the repo health Dashboard; otherwise skip this step. Each machine keeps its own Dashboard link.
   - Run `uv run careerbrew report public --out <scratch folder>` (never under `private/`; the command refuses it). Upload only when it exits 0 and prints three `wrote` lines. On any other result (`public export refused`, an error), publish nothing and list it as a problem (last step).
   - Read the `checkup_artifact_url` line of `uv run careerbrew config`. If it is empty, create the Dashboard: find the Dashboard type with the Artifact tool's `quickstart` (`intent: "other"`), publish with its `type_url` and the title `careerbrew repo health`, then save the link it returns: `uv run careerbrew config set checkup_artifact_url <link>`.
   - Upload `summary.json`, `steps.json`, and `daily.json` to that link as assets (one call each). On a new Dashboard, write in one batch: `dash/meta` (`{"title": "careerbrew repo health"}`), one `datasets/<id>` per entry of `config/repo-health-dashboard/datasets.json` (its `title` and `description`, `source: {kind: "file", url: <the asset's url>, name: <file>, format: "json"}`, and `updated: {at: <now>, by: "careerbrew report public"}`), and `files/index.html` with the text of `config/repo-health-dashboard/index.html` unchanged. On an existing one, update only each dataset's `source.url` and `updated`, then delete the old assets.
   - Upload only those three files. Never publish a run page, a log, or anything else from `private/`.
4. Answer first, in two or three lines: pass or fail, the open pull requests (always, even when the rest passes; an open PR comes before new work), and the first blocker with its fix. If a blocker says the open pull requests could not be listed, say so and rerun the checkup outside the sandbox; never report "no open PRs" from a failed check. Before you tell the user to continue the open-work branches, check them against merged PRs (`gh pr list --state merged --json headRefName`): a branch of a merged PR is not open work, and a list of them is a problem to file (last step), not work to continue. A never-pushed branch whose work you confirmed landed elsewhere is retired with the `git tag archive/<branch> <branch>` command its line names (no branch delete; a new commit lists it again).
5. Ask the user which next actions to take, with the page's Next items as the options (each option: its label and its command; offer the first four). Run only what the user picks.

Do not fix anything unless the user asks.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Last step: file every problem you found

List every problem you found in careerbrew or your setup during this run: a failed check, a fix line that was wrong or confusing, a check that cannot pass, a step you had to work around. If there are none, say `Problems found: none` and stop.

Skip a problem that is already tracked: an open PR, a branch, or a task that the checkup output names. A problem only on this machine that its own fix line solved needs no issue; list it with that fix.

For each other problem, in this order:

1. **Pick the repo.** A careerbrew problem (its code, docs, checks, or fix lines) goes to this repo, `fyodoriv/careerbrew`. A problem in another tool (agentbrew, dotfiles, a skill) goes to that tool's repo through the `report-upstream` skill, which does the rest. A problem in a company plugin goes to the plugin's own repo, never here. You are the owner when `gh api user -q .login` prints the same name as `gh repo view fyodoriv/careerbrew --json owner -q .owner.login`.
2. **Search for a duplicate first.** Run `gh issue list -R fyodoriv/careerbrew --state all --search "<2-4 key words>" --limit 10 --json number,title,state,url > <file>; rc=$?`, and `rg -n -i "<key word>" TASKS.md`.
   - A nonzero `rc` is a failed search, not "no duplicate". `gh` often fails in the sandbox with a TLS error, so rerun it once outside the sandbox. If it fails again, GitHub is out of reach: use the fallback in step 5.
   - An open issue or task matches: do not file a new one. Report its id. If this run adds a fact it lacks (a new error line, another machine), send that fact as a comment through steps 3 to 5 (`gh issue comment <N> -R fyodoriv/careerbrew --body-file <draft>.md`).
   - Only a closed issue matches: the problem came back. File a new one whose body starts with `Came back after #<N>.`
3. **Draft it without private data.** In a temporary folder outside `private/`, write the title to `<draft>.title` (`<area>: <what is wrong>`) and the body to `<draft>.md`: `## Problem` (what ran and the exact failing line), `## Expected`, and the commit (`git rev-parse --short HEAD`). Describe the careerbrew command and its error only. Never include anything from `private/` (career, job, or work data), not even scrubbed. Write `~` for your home folder. The owner's body ends with a `---` line and `_🤖 Written by an agent, not Fyodor. Ping me if this looks off._`.
4. **Check the draft.** Run `uv run careerbrew privacy scan --file <draft>.title --file <draft>.md`. Go on only when it prints `0 finding(s)`. A finding names a term id or a path rule, never the term: rewrite that part and scan again. Never post a draft that has a finding.
5. **File it.**
   - The owner: `gh issue create -R fyodoriv/careerbrew --title "<title>" --body-file <draft>.md`. `/careerbrew-grind` works through the owner's issues.
   - Anyone else: follow the `report-upstream` skill. Run `agentbrew report --title "<title>" --body-file <draft>.md --repo fyodoriv/careerbrew` (no `--yes`), show the user the safety checklist and the exact text, and post only after they say yes. A comment on a duplicate needs their yes too.
   - GitHub out of reach (the search or the post failed twice): the owner must add a task to `TASKS.md` instead, in its format (ID, Details, Hypothesis, Measurement, Success, Pivot, Anchor), with "check for a duplicate issue first" in Details. Leave it uncommitted and say so. Anyone else saves the draft with `agentbrew report` (no `--yes`).

Then report one line per problem, with its id: `filed #<N>`, `duplicate of #<N>`, `task <ID>`, or `not filed: <reason>` with the draft path (for example `not filed: could not search for duplicates`).
