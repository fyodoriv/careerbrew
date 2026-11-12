---
name: careerbrew-project-explainer
description: Explains a software project or repo in layers, with a citation on every claim, and ships the doc only when the checker passes. Deep mode also runs the project, checks the doc, offers a PR, and drafts a PRD (requirements, decisions, estimates in dev weeks, and pushback when the plan slot is too small). Use when the user says "explain project X", "give me the five-minute version", "what is X and where is its code", "run it and explain it", "write a PRD for X", or "is this plan big enough". Don't use for teaching and quizzing over days (use careerbrew-learn-project), for pushing a tracked project forward (use careerbrew-project-push), or for interview practice (use interview-drill or teacher).
---

# Explain a project

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

Run every command from the careerbrew repo
(`careerbrew …` from anywhere else; `scripts/setup.sh` installs that command). Docs go to
careerbrew's private `out/` folder, in plaintext by design (delete a doc there when you are done with it). Never write into the explained repo
unless the user says yes to a PR (Deep mode, step 4).

## Pick the mode

| The user says | Mode | Ends with |
|---|---|---|
| "explain X", "five-minute version", "where is the code for X" | **Base** | one cited doc that passes `work project explain --check` |
| "run it and explain it", "deep dive", "explain it and open a PR" | **Deep** | a deep doc that passes `--deep --check`, a PR offer, and a PRD draft when asked |
| "write a PRD", "is this plan big enough", "estimate this" | **PRD** | a PRD that passes `work project prd --check` |
| "teach me", "quiz me", "onboard me over a few days" | hand off to `careerbrew-learn-project` | |
| unclear | ask one question: "a five-minute read, a deep run, or a PRD?" | |

## Company data stays in plugin config

- Project names come from `careerbrew work project list`; source hosts come from the company plugin (`connector_defaults`, `agent_reads` in `careerbrew-plugin.yaml`, `docs/plugins.md`).
- Never write a company name, project name, or internal host into this skill, an eval, or a doc in the careerbrew repo. Examples use `*.example.com` and `PROJ-123`.
- On a corporate machine, read business sources only as the plugin policy allows (`agent_may_read_business`).

## Base

1. Find the reading list. A tracked project: `careerbrew work project show <name>`. A bare repo path: its `AGENTS.md`, README, and top-level code folders. If the request names neither, or matches more than one project, ask one question and stop.
2. Write the outline: `careerbrew work project explain <name> [--short]` (a repo path: copy `references/example-careerbrew.md`).
3. Re-read every source now. Record the read date or commit sha, and stamp it with `careerbrew work project check <name> <ref>`. When sources disagree, the primary source wins; list the losers under Gotchas as stale.
4. Fill each section with the rules in `references/explain-method.md`: one claim per bullet or table row, a `[S<n>]` citation inline on each one, each acronym defined at first use, at most 4 new terms per section.
5. Run `careerbrew work project explain <name> --check <file> [--short]` until it prints `explain check: pass`.
6. Grade it: an independent subagent checks that every quote matches its source word for word and that every absence claim names the check that proved it. The checker cannot see these.
7. Report the file path, the checker result, and the grader's findings fixed.

The checker blocks: headings out of order, a bullet or table row with no citation, a source with no kind, locator, or read date, a claim backed only by leads, a Containers table with no Deploys as column, a Why table whose State is not decided or proposed, a one-minute version that is not 3 flat bullets, and an acronym not defined at first use.

## Deep mode

1. **Run the project.** Use the repo's one start command (its README or `AGENTS.md`), then its health check and fast tests. Save what you ran and what it printed to a file in `$TMPDIR`; cite it as a source (`path` locator, today's date). A start that fails is a finding: write it under Gotchas with the exact error, and never repair the repo's setup without a yes.
2. **Explain it.** Write the outline with `careerbrew work project explain <name> --deep`. It adds **How it runs** after the codemap: one bullet per step, the command in backticks, what it showed, and a citation.
3. **Check it.** Run `careerbrew work project explain <name> --deep --check <file>` until it passes, then the grader of Base step 6.
4. **Offer a PR.** Ask once: "add this doc to the repo as a PR?" On a yes, and only on a repo the user may change: run `careerbrew privacy scan --file <doc>` first, branch, add the doc under the repo's docs folder, and open the PR with that repo's delivery rules. A doc that names company data never goes into the careerbrew repo.
5. **Draft a PRD** when the user asks for a plan, or the run found work to do: follow the PRD rules below.

## PRD rules

A product requirements document (PRD) says what to build, why, and how big it is.

1. Write the outline: `careerbrew work project prd <name> [--slot N]`, where `N` is the plan slot in dev weeks. The slot is the time the plan gives the work; ask for it when the user did not say.
2. Fill the sections in this order: **Problem**, **Requirements**, **Decisions**, **Pushback** (when needed), **Open questions**, **Sources**. See `references/example-prd.md`.
3. **Requirements**: a table with Id, Requirement, Estimate, and Source. Each estimate is `N dev weeks` (one engineer, full time). Never days, sprints, points, or hours: convert them, and say how in the Pushback or a source.
4. **Decisions**: a table with State, Who, and When. State is `decided` or `proposed`; "considering" in a source means proposed.
5. **Pushback**: when the estimates add up to more than the plan slot, say so plainly, with both numbers in dev weeks, and name what to cut or how much time to add. Do not shrink the estimates to fit the slot.
6. **Open questions**: each one names who can answer (`Who: ...`).
7. Every bullet and table row cites a source, as in Base.
8. Run `careerbrew work project prd <name> --check <file>` until it prints `prd check: pass`. Then report the path, the total estimate against the slot, and the open questions.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-project-explainer/LESSONS.md`: what worked, what failed, and why. If the behaviour should change, update this skill, the checkers in `src/careerbrew/work/explain.py` and `src/careerbrew/work/prd.py`, and a case in `evals/cases/023-project-explainer.yaml`.
