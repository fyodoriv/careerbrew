---
description: Learn a project, repo, or body of knowledge so it sticks - a cited five-minute explanation, or teach, quiz, spaced reviews, and retests (careerbrew-learn-project skill)
---

Load `$(careerbrew where)/skills/careerbrew-learn-project/SKILL.md` and follow it. `$ARGUMENTS` names what to learn (a repo path, a tracked project, or a topic) and may say the depth ("quick" or "deep").

Run careerbrew from anywhere as `careerbrew …` (the launcher `scripts/setup.sh` installs). Learning state lives in careerbrew's private data folder (`careerbrew learn status <project>`), never in the repo being learned.

Start with `careerbrew learn list`. If the project is already there, resume at `careerbrew learn next <project>`.

End each session with `careerbrew learn status <project>`: known % per section, what is due and when, the +7-day retest result, and the Brier score.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.
