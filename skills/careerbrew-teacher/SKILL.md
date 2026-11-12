---
name: careerbrew-teacher
description: Keeps careerbrew's teacher mode current. It reads predicted and written feedback, fixes lessons, hints, pitfalls, exercises and cards before the owner has to ask, refreshes the teaching rules from best practice, and checks the step order still serves the owner's upcoming goal. Use when the user says "improve the teacher", "update teacher mode", "the teacher is wrong about X", after a goal change, or weekly. Don't use to set goals, choose jobs, send anything, or approve on the owner's behalf; only the owner does those.
---

# Teacher

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. `uv run careerbrew goals show` and `uv run careerbrew teach next`. The upcoming goal belongs to the owner: read it, never change it. If there is none, suggest one in chat and point to Goals in the web app.
2. `uv run careerbrew teach improvements`. Every line is a predicted or written complaint with a proposed fix. Lines marked `applied:` were already fixed automatically by the teacher (watch-out tips, worked examples, lesson examples, suspended leeches); check the fix worked and fix the root cause in the content. Fix each other line at its source:
   - `stuck-exercise`, `low-confirm`: rewrite the exercise's `lesson` (an analogy plus a tiny example) or add an easier warm-up exercise before it in the same pattern (`knowledge/exercises/`), then `uv run careerbrew exercises check`.
   - `repeat-mistake`: add a `pitfalls:` tip for that kind to the pattern's exercises; make the tip specific to the pattern.
   - `slow-step`, `skipped`: change the step order or size in `src/careerbrew/teacher.py` with a test in `tests/unit/test_teacher.py`.
   - `card-leech`: rewrite the card smaller, or improve its public explanation with the `careerbrew-card-explainer` skill.
   - `written`: treat the owner's words as a defect report; fix it and say what changed.
3. Best practices: once a month, check the rules in `knowledge/teaching.yaml` against current learning-science and interview-prep sources (use `careerbrew-browser-research`; every new source goes through `knowledge/research/` and `scripts/check-sources.py`). Change a rule only with a source or a measured reason; the rules hash in every teacher log row shows its effect.
4. Agents never decide approvals, set goals, or choose jobs. To ask the owner for something, run `uv run careerbrew approvals request "<title>" --detail "<what and why>"` and stop; act only when `uv run careerbrew approvals show <id>` exits 0.
5. Run `scripts/check.sh fast`, commit public content changes (exercises, teaching rules, code); nothing from `private/`.
6. Report counts only: proposals fixed, rules changed, exercises changed.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-teacher/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
