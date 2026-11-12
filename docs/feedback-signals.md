# Feedback signals: what each of your decisions changes

careerbrew stores what you decide, and reads it back so a later decision changes (issue #185). `careerbrew signals [--json]` lists each signal your decisions made and what it changes now; `careerbrew signals reset <kind> <source>` forgets one signal's decisions so far. The reader is `src/careerbrew/signals.py`.

Only decisions from the last 90 days count, so an old run of dismissals fades. One Accept all or Dismiss all counts as one decision, however many rows it decided.

| Your input | Where it is stored | What it changes |
|---|---|---|
| Dismiss a proposal (a plugin's suggestion, a proposed task, a source) | `day/inbox.json` (`status`, `decided_at`) | Proposals of that kind from that source (a plugin, or a project) rank lower in the inbox: weight `2 × (accepted + 1) / (accepted + dismissed + 2)`, between 0.1 and 1.0. No decisions: weight 1.0, order unchanged |
| Accept a proposal, or answer a question | `day/inbox.json` | Raises that kind and source's weight back toward 1.0, and ends a run of skips |
| Skip (dismiss) a question | `day/inbox.json` | New questions fill `questions_per_day` highest weight first; after 3 skips in a row of one kind about one source, no new question of that kind is asked about it for 14 days; then one comes through again (answer it to end the run). The source is the project (where notes go), the tool (an impact question about a `github` PR or a `jira` ticket; `pr` counts as `github`), the ref kind (which project a `tracker` key or a `repo` is part of), or who added the task (a competency question) |
| Decide a plugin's suggestions | `day/inbox.json` | `careerbrew day suggestions` gives the plugin the `weight` its next suggestions get, so it can send fewer of what you dismiss |
| Edit and save the PM update | `day/pm-edits.jsonl` (the raw draft beside your saved text, last 14 edited days) | A project line you cut from the shown draft on 2 or more days, more often than you keep it, is left out (typing it back counts double against the cut, and the PM view names what is left out); only what you change from the shown draft counts, so a lesson never confirms itself; a label you rename (same text after the colon) on 2 days is written your way; a part (`next:`, `overdue:`) you cut from a line on 2 days is dropped; lines no project rank orders follow the order you put them in. Ranked projects keep their rank. Reset to draft drops that day's edit from the lessons; `careerbrew signals reset cut <label>` (or `rename`, `part`) forgets one label's lessons up to that moment: an edit you save later the same day counts again |
| Send the PM update (issue #238) | `day/pm-sent-<day>.md`, optional `day/pm-sent-receipt-<day>.json`, and `day/pm-edits.jsonl` (`from: sent`) | A manual `day pm-sent` record stays authoritative. An explicitly approved app send follows only its receipt identity; otherwise careerbrew matches an own channel post or exact-recipient DM. The watcher revisits created days within feed retention, even without new results ([edit and recipient rules](#edits-to-sent-pm-messages)). Sent text wins over a web save and teaches the same lessons; a kind of line you add that the draft missed (`meeting`, `announcement`, `own post`, `other`) on 2 days shows in `careerbrew signals`. The learner only reads; Send still requires approval |
| Set an upcoming goal with topics | `goals.yaml` (`card_topics` from the goal's kind and topics) | New flashcards on the goal's topics come first in Teach me and the card queue; due reviews keep their order |
| Write a long-term goal | `goals.yaml` (`long_term`) | A ladder competency the goal names (a word of 4+ letters from its id or name) has its gap priority raised 1.5 times, so it ranks higher in What is missing and in goal suggestions |
| "Drop it" when checking a card you wrote | `teach/state.json` (`dropped_cards`) and the card's suspended flag | The card leaves review for good; `careerbrew cards unsuspend <id>` brings it back |
| See the reference solution after three failed tries | `teach/state.json` (`retry_later`) | The exercise comes back in Teach me 3 days after the last try, until you solve it |
| Rate a Teach me step 2 or lower (twice in 30 days, one step kind) | `teach/feedback.jsonl` | Low-rated card steps move after exercises: Teach me offers the next exercise before due cards |
| `careerbrew work task-set <id> --impact --urgency --hours` | `work/tasks.yaml` | An accepted task starts at impact 3, urgency 3, 1 hour; your grade replaces it, and the ranks and the plan use it |
| Finish a task (`work done <id> <hours>`) | `metrics.jsonl` (`estimate_ratio` with the task and its source) | The plan stretches each task's estimate by its project's own ratio once 3 of its tasks are done, else its source's (yours, an agent's, a proposal's), else everyone's median |
| Snooze a notification | `day/notifications.json` (`snoozes`) | 3 snoozes of one kind (a DM, a mention, a suggestion kind) in 14 days lower that kind's priority by one |
| A note on an approval decision | `approvals.json` (`note`) | Standing guidance: `careerbrew approvals request` prints your last notes to the agent before it asks again; `careerbrew approvals guidance` lists them |
| Rate a posting with tags (`jobs rate <id> <1-5> --tags ...`) | `jobs/ratings.jsonl` | Every 5th rating refits your Want weights (kept only when they agree with you better); a tag you put on 2 low ratings (2 or less) lowers postings that name it by 15% each, a tag on 2 high ratings raises them by 10% |
| Not an ask, Done (Today's open asks, or `work feed mark <id> not-ask\|done`) | `work/feed/events.jsonl` (`ask_mark`) | The ask leaves your open asks and stays closed after later pulls; `ask` opens it again. 3 not-ask marks, or dismissed ask notifications, from one channel or sender propose a `triage: fyi` rule for it in the inbox |
| Mark a feed event personal | `work/feed/events.jsonl` (`marked`) | 3 personal marks from one channel or sender propose a `personal: true` rule for it in the inbox |

## Per-line PM edit candidates

Each changed line between the shown PM draft and the owner's saved text yields a
candidate instruction (`pm_learn.rules()["edit_rules"]`, issue #336). Added,
removed and replaced lines are covered; unchanged shown lines teach nothing,
even when learning already changed the raw draft. The existing encrypted history
is the source of truth. New candidate instructions are kept with the saved edit in that same encrypted
history, under its existing limit of 14 edited days. The privacy-gated data CLI can show
them only where business reading is allowed; `--keys` shows types only. No new
raw-text CLI or API exit is added. The source context must match before reuse.
Automatic cut, rename and part lessons retain the two-edited-day threshold.

## Edits to sent PM messages

The watcher reconciles sent text even when a cycle adds no result rows. A Slack
message edit replaces its feed original. Learning uses the message's created
day in your local time zone, including older days whose own Slack originals
remain within feed retention. Several edits on one day still count as one
edited day. An unchanged repeat does not rewrite PM history.

For `pm_slack` set to a member id, a feed-discovered update requires an own,
non-personal, one-to-one DM whose original channel metadata names that exact
member id. Missing recipient metadata and group DMs are ignored. No new Slack
permission or history request is added.

An approved `day send --yes` records the channel and timestamp returned by
Slack as an encrypted receipt. Later edits can replace that saved source only
when the feed contains the same own message. Similar text in another message
cannot replace it. Learning keeps the latest text as the fallback after feed
retention expires. `day pm-sent <file>` remains authoritative and clears any
app receipt for that day. Older sends with no receipt retain that authority;
they are never matched by guesswork. These reads send no messages.
Malformed receipts are preserved and ignored. The event log records only an
`InvalidReceipt` reason. DM type flags must be actual booleans.

Run the aggregate audit from your terminal or with the business-read policy.
Use `uv run python scripts/pm-sent-coverage.py --from YYYY-MM-DD --to YYYY-MM-DD --json`.
It counts edited sent days with an identified source and a shown draft, and
checks for matching sent history on the same day. Unchanged drafts and
ambiguous sources are excluded. An empty denominator gives an insufficient
data result. A concurrent change gives an incomplete result.

The additive `defects` counts apply only to eligible days. `missing_history`
means no same-day sent row and no matching body elsewhere. `stale_history`
means the latest same-day sent body differs. `wrong_day_history` means no
same-day sent row but a matching body exists elsewhere, including outside
the requested window. Repeated bodies mean this flag cannot prove an intended
date. These three counts are exclusive. `duplicate_days` counts eligible days
with multiple sent rows; the last stored row still decides coverage. A covered
duplicate day keeps coverage credit but fails the later zero-duplicate criterion.

The two-week real-use coverage check is still pending (`pm-sent-edits-and-dms`
in `TASKS.md`). Use the content-free coverage audit for this observation;
current signal summaries and data key names do not measure that coverage.
An unchanged sent draft remains a no-op and creates no empty lesson.
