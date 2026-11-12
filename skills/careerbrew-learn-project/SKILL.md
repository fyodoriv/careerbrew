---
name: careerbrew-learn-project
description: Teaches you a software project, repo, or body of knowledge so it sticks, and measures that it stuck. Two depths. Quick hands off to the careerbrew-project-explainer skill (a cited five-minute explanation). Deep stages the sources, builds cited concepts, then runs predict, read, explain, and quiz rounds with spaced reviews and retests at +1 and +7 days, all kept by `careerbrew learn`. Use when the user says "teach me this project", "onboard me on X", "tutor me", or "quiz me on X". Don't use for a one-off explanation, a deep run, or a PRD (use careerbrew-project-explainer), for interview practice (use interview-drill or teacher), or to push a tracked project forward (use careerbrew-project-push).
---

# Learn a project

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

careerbrew keeps all learning state under `learn/<project>/` in its private data
folder, encrypted. Never write notes, decks, or staging folders into the repo being
learned. Run every command from the careerbrew repo
(`careerbrew …` from anywhere else; `scripts/setup.sh` installs that command).

## Pick the depth

| The user says | Depth | Ends with |
|---|---|---|
| "explain X", "five-minute version", "what is X and where is its code" | **Quick** | one cited doc that passes `work project explain --check` |
| "teach me", "onboard me", "tutor me", "quiz me", "learn this repo" | **Deep** | concepts mastered, reviews scheduled, retests due |
| unclear | ask one question: "a five-minute read, or teach and quiz you over a few days?" | |

A quick doc is a good first step of a deep run: offer it as the overview.

## Quick depth

Load the `careerbrew-project-explainer` skill and follow its Base mode. It writes the cited
doc and runs the checker (`careerbrew work project explain`). Its deep mode runs the
project and drafts a PRD; that is not this skill's deep depth, which teaches and quizzes.

## Deep depth

Run `careerbrew learn status <project>` first. If the project exists, resume at `careerbrew learn next <project>`; never restart unless the user says "start over".

### 1. Stage the sources

Infer what you can (inside a repo, "teach me this project" means this repo). Ask once, batched, for the rest. Never invent a private repo name, doc id, or ticket key.

- Local repos: `--repo PATH`. They are read in place, by path and commit.
- Anything you fetch (a doc with all its tabs, a PR with its changed files, a ticket with its children, a web page): use whichever connector or tool is connected, read its schema first, save the text to a file in `$TMPDIR`, then pass it with `--doc LABEL:FILE`, `--context LABEL:FILE`, `--web URL:FILE`, or `--file LABEL:FILE`. Agent-provider gate: before you read work data with your own tools, follow rule 1 of `careerbrew-best-practices`.
- Always add `--consume`: careerbrew stores the text encrypted and deletes your plaintext copy.

```
careerbrew learn start <project> --repo ~/apps/svc --doc "Design:$TMPDIR/design.md" --consume
```

A source that fails to fetch is skipped and named in your summary. Never repair a connector. `start` prints which sources changed since last time; re-teach only concepts that cite them.

### 2. Build cited concepts

1. Read the sources. Write study notes with `careerbrew learn note <project> <NN-section>/<topic>.md --set FILE --consume`. Every claim cites a staged source or is left out (cite or refuse).
2. Write `concepts.yaml` (shape in `references/pedagogy.md` §5): each concept has a section, a title, prerequisites, citations, and questions with a Bloom level, a prompt, and an answer. Tag each wrong option with the misconception it targets.
3. Import it: `careerbrew learn concepts <project> FILE --consume`. It refuses missing citations, prerequisite cycles, more than 30% pure-recall questions, and untagged distractors. Fix and rerun until it imports.
4. Verify each question from the notes only (pedagogy §1). Drop a question the notes cannot answer uniquely.
5. Offer the quick-depth doc or a short brief: the big picture first, then the few concepts that matter most, each linked to its note.

### 3. Practise, one step at a time

Loop: `careerbrew learn next <project> --json` names the mode, the concept, the Bloom level, and a question. Run that mode, then record the result:

```
careerbrew learn answer <project> <concept> --mode <mode> --result correct|partial|wrong|dontknow --confidence low|medium|high --level <bloom> [--kind open|mcq|task] [--seconds N]
```

| Mode | What you do |
|---|---|
| `pretest` | Ask before teaching: "predict how X works". Not scored, so say a miss is expected. Then teach. |
| `teach` | Show the note in short pieces. After each piece ask "explain in your words why this holds" (self-explanation), then one question. |
| `quiz` | Open recall by default: a "why" or "what happens when" question, graded by a hidden 2 to 4 point rubric (pedagogy §2). Multiple choice only for a first look at a section. |
| `task` | An applied task on the real repo: locate the owner, where to add, trace the bug, blast radius. Graded against real paths. |
| `review` | A spaced review FSRS scheduled. Ask in new words, never the same prompt. |
| `retest` | A delayed retest of a mastered concept (+1 day, +7 days). New words, same level. A miss here is the honest signal: teach again. |

Rules for every question (pedagogy §1 to §4):

- Ask for confidence before you reveal the result. High confidence and wrong is a misconception: name it, quote the note, and drill it first.
- Hints come in tiers, never the answer first: (1) point to the section or file, (2) name the concept, (3) show a worked example of a similar case, (4) reveal and record `dontknow`.
- Always offer "I don't know, teach me". Record it as `dontknow`, not a wrong guess.
- Test meaning, not labels or ids. One correct option, placed at random. Clean, short English.
- Once each section's core concepts are mastered, mix sections (interleave) and say so.

Mastered means two correct answers that were not low-confidence, one at Bloom `apply` or higher. `answer` prints it and schedules the retests.

### Hands-on: replay a PR, walk a tour

- When `learn next` says `task`, or a section is mastered, offer a replay: `careerbrew learn replay list <project>`, pick a PR that touches the section, then `careerbrew learn replay start <project> <pr>`. Give the PR title as the brief, never the diff. The learner edits the clone it names; `careerbrew learn replay check <project> <pr>` grades it with the PR's own tests. Record it with `learn answer ... --mode task --kind task --level apply` on the concept it exercised.
- For a first look at a section, offer a code tour: write the tour JSON (each step a file, a line, a short explanation, and its concept), add it with `careerbrew learn tour add <project> <name> FILE --consume`, and export it with `careerbrew learn tour export <project> <name>`. Tell the user a tour is unreviewed until a maintainer runs `learn tour approve`. Mastered steps fade to a prediction on the next export.

### 4. Report

After each session print `careerbrew learn status <project>`: known % and remaining concepts per section, due reviews and retests, 7-day retention, the retest result at +7 days, and the Brier score (confidence against results; lower is better). Tell the user when the next review or retest is due.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-learn-project/LESSONS.md`: what worked, what failed, and why. If the behaviour should change, update this skill, `src/careerbrew/learn/project.py` and a case in `evals/cases/`.
