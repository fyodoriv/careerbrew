# Learn a project

`careerbrew learn` keeps what you study about a project, what you get right, and what is
due next, so the knowledge lasts past one chat. The `careerbrew-learn-project` skill teaches; this
command keeps the state. Agents start it with `/careerbrew-learn`.

## Two depths

| Depth | When | What you get |
|---|---|---|
| Quick | "explain X", "the five-minute version" | one cited doc that passes `work project explain --check` (US-56); the `careerbrew-project-explainer` skill writes it |
| Deep | "teach me", "onboard me", "quiz me" | staged sources, cited concepts, a practice loop across days (US-80) |

## The deep loop

```
careerbrew learn start <project> --repo PATH --doc "Design:$TMPDIR/design.md" --consume
careerbrew learn note <project> 01-core/queue.md --set FILE --consume
careerbrew learn concepts <project> concepts.yaml --consume
careerbrew learn next <project>            # pretest, teach, quiz, task, review, or retest
careerbrew learn answer <project> <concept> --result correct --confidence high --level apply
careerbrew learn status <project>          # known % per section, what is due, retest and calibration
careerbrew learn list
```

`learn next` picks, in this order: a due retest, a due spaced review, a pretest for an
untouched section, then the weakest section's next concept whose prerequisites are mastered.

- A missed retest or review takes mastery away. Only answers after the miss count again, and
  re-mastery schedules new retests.
- A card forgotten 8 times (a leech) is not suspended here: answering it again is the re-teach,
  and `learn answer` says to rewrite its note and questions.
- `status` lists as stale only sources that changed after they were taught, until the next
  `learn concepts` import. Re-importing concepts drops the retests of removed concepts.

## Replay a PR

`learn replay list <project>` lists the newest small squash-merged PRs (at most 12 files
and 500 changed lines) in the project's staged repo. `learn replay start <project> <pr>`
makes a shared clone at the PR's parent with only the PR's tests brought in; you make them
pass, and `learn replay check` grades it. The PR title is the brief; the diff stays hidden.
Doing the real change is the strongest test of understanding: it is the `apply` and
`analyze` levels with the project's own grader.

`learn replay measure --repo PATH` reports how many of the last 50 small PRs make a
runnable task: their tests fail before the PR and pass after it. Python tests only for now.

## Code tours

A tour is JSON with a staged `repo`, a `title`, and `steps` (`file`, `line`,
`description`, and an optional `concept`). `learn tour add` checks every file and line.
`learn tour export` writes a [CodeTour](https://github.com/microsoft/codetour) file to
`private/out/tours/`; open it in your editor with "CodeTour: Open Tour File" from the repo's
folder. A step whose concept you mastered asks you to predict instead of explaining
(fading worked examples, Renkl & Atkinson 2003). A maintainer runs `learn tour approve
--by NAME`; once a toured file changes after that commit, the export says so on step 1.

## What it measures

| Number | Meaning | Why |
|---|---|---|
| `scheduled_share` | taught concepts with a spaced review scheduled | forgetting is steep without spacing (Cepeda et al. 2006) |
| `retest_retained_7d` | mastered concepts still right at +7 days | only a delayed test shows learning (Soderstrom & Bjork 2015) |
| `retention_7d` | FSRS reviews recalled in the last 7 days | the scheduler's own target |
| `brier` | gap between your confidence and your results (0 is perfect) | high-confidence errors are the ones to fix first (Butterfield & Metcalfe 2001) |
| known % per section | concepts mastered / concepts | coverage, not accuracy |

Mastered means two correct answers that were not low-confidence, one at Bloom `apply` or
higher. A Bayesian Knowledge Tracing model was tested against this rule before it shipped
and lost (the check is pinned in `tests/unit/test_learn.py`).

The rest of the method, with sources, is in `skills/careerbrew-learn-project/references/pedagogy.md`.

## Where the data lives

Everything is under `private/learn/<project>/`, encrypted ([privacy.md](privacy.md)).
Nothing is written into the repo you learn. Learn cards have their own deck per project, so
they never mix with interview flashcards, and `learn` works on a corporate machine.
