# careerbrew: explained

## One-minute version

- What: a private, local-first career assistant for programmers, run from the command line and a coding agent [S1].
- Why: it keeps a measured, private model of the owner that drives both the job hunt and promotion work [S2].
- Status: in use on `main`; success is at least 75% of outcome metrics in KEEP (met its target) state after 8 weeks [S2].

## Context map (C4 level 1)

| Actor or system | Talks to careerbrew how | Source |
|---|---|---|
| The owner | runs `careerbrew` in a terminal and reads the web app [S1] | [S1] |
| A coding agent | runs `/careerbrew` and discovers actions with `careerbrew actions --json` [S1] | [S1] |
| Work tools | read through connectors in `src/careerbrew/connectors.py` [S1] | [S1] |
| Model providers | named only in `config/models.yaml`, local route first [S1] | [S1] |

## Containers (C4 level 2)

| Container | Job | Deploys as | Source |
|---|---|---|---|
| Command-line app | every action, built from one argument parser [S3] | Python package run with `uv run careerbrew` [S1] | [S1] [S3] |
| Local JSON (JavaScript Object Notation) server | the data behind the web app, bound to localhost [S4] | process started by `careerbrew serve` [S4] | [S4] |
| Web app | views over the same actions [S1] | TypeScript build served from `web/dist` [S4] | [S1] [S4] |
| Data folder | personal data, encrypted at rest [S1] | files under `private/` [S1] | [S1] |

## Codemap

- Where actions are listed: `actions.py` reads the argument parser, so a new command is an action at once [S1].
- Where project records live: `work/projects.py` (sources, read dates, stale flags) [S5].
- Where this explanation is checked: `work/explain.py` [S5].
- Where encrypted reads and writes happen: `store.py` (`read_text`, `write_text`) [S1].

## Invariants and boundaries

- Personal and business data stay in `private/`; tests use synthetic canaries only [S1].
- Every action runs three ways: command line, agent, and web app; a parity test enforces it [S1].
- New code uses the pinned clock, so tests freeze time [S1].

## Why it is this way

| Choice | State | Who | When | Source |
|---|---|---|---|---|
| Local-first, models named in one file | decided | the owner | 2026-10-18 | [S1] |
| A rule a machine can check becomes a gate step | decided | the owner | 2026-10-18 | [S1] |

## Current state

- Open: which outcome metrics reach KEEP after 8 weeks of use? Who: the owner, from `careerbrew review` [S2].

## Gotchas

- Read data files with `store.read_text`, never a plain file read: they are encrypted [S1].
- After the clone moves, run `uv sync --reinstall`, because virtual-environment scripts keep old absolute paths [S1].

## Read next

- `AGENTS.md`: the routing table for every change [S1].
- `docs/VISION.md`: the non-goals and kill criteria [S2].

## Check yourself

- Where do you add a model name? [S1]
  <details>In `config/models.yaml` only.</details>
- How does a new command reach the web app? [S1]
  <details>`actions.py` reads the parser, so it is listed at once.</details>
- What does a bug fix start with? [S1]
  <details>A failing test that reproduces it.</details>

## Glossary

- KEEP: an outcome metric that met its target in `careerbrew review` [S2].
- Pinned clock: a clock tests can freeze through an environment variable [S1].

## Sources

| Id | Kind | Locator | Read |
|---|---|---|---|
| S1 | primary | AGENTS.md@c334531 | c334531 |
| S2 | primary | docs/VISION.md@c334531 | c334531 |
| S3 | primary | src/careerbrew/actions.py@c334531 | c334531 |
| S4 | primary | src/careerbrew/server.py@c334531 | c334531 |
| S5 | primary | src/careerbrew/work/ | 2026-10-21 |
