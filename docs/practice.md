# Practice: flashcards for interview knowledge

careerbrew has two ways to practise:

- **Drill** (`careerbrew drill`): open questions graded with binary rubrics, for coding, system design and behavioural answers.
- **Cards** (`careerbrew cards`, web **Practice** view): short question and answer flashcards, for facts and concepts you must recall fast.

Both use spaced repetition. Cards come from your own decks (`careerbrew cards add`) and from imports (the Notion import fills the `notion` deck).

## How a session works

1. Open **Practice** (or run `careerbrew cards review`).
2. Read the question and answer it from memory, out loud or on paper.
3. Press Space to see the answer.
4. Grade yourself with keys 1–4: again, hard, good, easy.
5. The next card appears. Due reviews come first, then new cards.

## The practices behind it

| Practice | What careerbrew does | Source |
|---|---|---|
| Retrieval practice | the answer stays hidden until you recall it | Roediger & Karpicke 2006 (`doi-roediger-karpicke-2006-test-enhanced`); Roediger & Butler 2011 |
| Spaced repetition | FSRS picks each card's next review from your grade | Cepeda et al. 2006 (`doi-cepeda-pashler-vul-wixted-rohrer-200`); ABC of FSRS (`github-abc-of-fsrs-open-spaced-repetitio`) |
| Interleaving | consecutive cards come from different topics | Rohrer & Taylor 2007 (`rohrer-taylor-2007-interleaving`) |
| Daily new-card limit | at most `cards_new_per_day` new cards a day (settings.yaml, default 20) | Anki Manual, Daily Limits (`anki-manual-daily-limits`) |
| Leech handling | a card you forget 8 times after you had learned it (failing while still learning does not count) is suspended until you rewrite it; after an unsuspend, the next warning comes after 4 more lapses | Anki Manual, Leeches (`anki-manual-leeches`) |
| Small cards | one fact per card; rewrite leeches smaller | Wozniak, Twenty rules (`wozniak-twenty-rules`) |

Full citations and URLs are in `knowledge/sources.yaml`.

"Today" (the daily new-card cap, reviewed today, the streak) is your local calendar day. A streak still counts yesterday until today ends.

In the terminal, type `q` (or press Ctrl-D) at any prompt to end a session.

## The public study deck

Every card can have a full explanation: the idea in plain words, how it works, an example, common mistakes, and what to say in the interview. Explanations live in `knowledge/cards/`, are public, and are shared by everyone who uses careerbrew; your own cards stay private and simply show the matching explanation. Questions that are personal (they name an employer or your own work) get an explanation too, but it is saved only in `private/` (encrypted) and never published. In the web app, open **Explanation** after you reveal the answer, or read a whole topic in **Library → Interview knowledge**.

The deck keeps improving: the `card-explainer` skill writes cards with `prompts/card-explainer.md`, `careerbrew cards publish` checks each card against the bar and for personal data, and `careerbrew cards explain-queue` brings cards back when the prompt improves, a card fails the bar, or learners keep grading it "again", or you report a problem with it ("Report a problem with this explanation" under every explanation in Practice and Library; reports stay in `private/`).

## Measures

- `careerbrew cards stats`: due now, new, reviewed today, 30-day retention, streak, leeches, per-topic counts.
- `retention` in `careerbrew review` counts card reviews and drill attempts. Target 0.8; below 0.6 the review says to shorten sessions.

## Commands

| Task | Command |
|---|---|
| Review in the terminal | `careerbrew cards review [--count 20]` |
| See the queue | `careerbrew cards next [--json]` |
| Grade one card (agents) | `careerbrew cards grade <id> again\|hard\|good\|easy` |
| Add a card | `careerbrew cards add "question" "answer" --tags "System design"` |
| Stats | `careerbrew cards stats` |
| Bring a leech back | `careerbrew cards unsuspend <id>` |

Cards are personal-only: a corporate-role machine refuses them, like the rest of interview prep.

## Coding exercise checks

`careerbrew exercises check` validates reference solutions and their scaling.
A failed scaling run now names the observed failure: wall timeout, signal exit,
nonzero exit, malformed result, scale execution error, or numeric growth rejection.
The diagnostic includes the test/scale stage, observed exit code or signal, and
configured wall, CPU, and memory limits. SIGKILL alone does not establish CPU
exhaustion or an out-of-memory cause. A configured memory limit is not proof that
the operating system enforced it.

These diagnostics do not change scaling thresholds, timing, or verdicts. They
exclude child output and exception text. Existing learner error fields stay
available. The check validates every exercise on each run. Old saved passes are not
reused because validation and runtime inputs are not fully bound.
