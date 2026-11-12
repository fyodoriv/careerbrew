# careerbrew: PRD for the project explainer in core

Plan slot: 1 dev week [S1]

## Problem

- The layered explainer lives inside the careerbrew-learn-project skill, so a user who asks only for an explanation loads the whole teaching loop [S2].
- A PRD draft has no checker, so its estimates and decisions go unchecked [S1].

## Requirements

| Id | Requirement | Estimate | Source |
|---|---|---|---|
| R1 | Move the explainer into its own core skill | 0.5 dev weeks | [S1] |
| R2 | Add a deep mode that runs the project before it explains it | 1 dev week | [S1] |
| R3 | Check a PRD draft with a deterministic checker | 1 dev week | [S1] |

## Decisions

| Decision | State | Who | When | Source |
|---|---|---|---|---|
| Company project names and source hosts stay in plugin config | decided | the owner | 2026-10-27 | [S1] |
| Estimates use dev weeks only | decided | the owner | 2026-10-27 | [S1] |
| Cut R2 when the slot stays 1 dev week | proposed | the owner | 2026-10-27 | [S1] |

## Pushback

- The requirements add up to 2.5 dev weeks, but the plan slot is 1 dev week: cut R2 and R3, or give the plan 1.5 more dev weeks [S1].

## Open questions

- Should a PRD also name its success metric? Who: the owner [S1].

## Sources

| Id | Kind | Locator | Read |
|---|---|---|---|
| S1 | primary | https://github.example.com/acme/shop/issues/223 | 2026-10-27 |
| S2 | primary | skills/careerbrew-learn-project/SKILL.md | 2026-10-27 |
