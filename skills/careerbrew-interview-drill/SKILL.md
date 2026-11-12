---
name: careerbrew-interview-drill
description: Coaches interview practice with spaced repetition (FSRS) and binary rubrics, then charts readiness. Teaches before the first attempt, withholds solutions until the user tries, and grades each rubric criterion with evidence. Use when the user says "drill", "practice", "mock interview", "quiz me", or asks how ready they are. Don't use for a quick factual question, on a corporate-role machine, or to pick companies (use careerbrew-job-hunt).
---

# Interview drill

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. `uv run careerbrew drill next --json`. In `learn` mode, teach the lesson first in 3–5 sentences.
2. Ask the question. Set a timer: coding 25 min, system design 35 min, behavioral 5 min.
3. **Withhold the solution** until the user has made a real attempt. Hints come in tiers: nudge, then approach, then solution, and only on request.
4. Grade each rubric criterion as met or not, quoting the part of the answer that shows it. Then run `uv run careerbrew drill answer <id> --met <ids> --seconds <n>`.
5. Give the feedback lines and one concrete drill for the weakest missed criterion.
6. Every 5 attempts, run `uv run careerbrew drill progress` and show the readiness trend.

For the user alone at a terminal: `uv run careerbrew drill session`.

## Voice practice

The user answers out loud; careerbrew transcribes on this machine and grades by the rubric.

1. Check `uv run careerbrew config` for `stt_command` or `stt_model`. If both are empty, give the setup line (`brew install whisper-cpp`, download a ggml model, `careerbrew config set stt_model <file>`). Never suggest a hosted speech API.
2. `uv run careerbrew drill speak <id> --audio <file> [--aloud]` (or `--text "<answer>"` when there is no microphone).
3. Show the transcript, then the met criteria and feedback lines. The keyword grader is a first pass: if the transcript clearly meets a criterion it missed, say so and re-grade with `drill answer <id> --met <ids>`.
4. In the web app: Interviews → Practice by voice.

## Flashcards

For short facts and concepts, use cards instead of rubric drills (see `docs/practice.md`):

1. `uv run careerbrew cards next --json` shows the queue: due reviews first, then today's new cards, mixed by topic.
2. Ask the question and **wait for the user's answer** before you show the card's answer.
3. Show the answer. Let the user grade: again, hard, good, easy. Then `uv run careerbrew cards grade <id> <rating>`.
4. If a card becomes a leech, help the user rewrite it smaller (one fact per card), then `careerbrew cards unsuspend <id>`.
5. The user alone: the web Practice view, or `uv run careerbrew cards review`.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-interview-drill/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
