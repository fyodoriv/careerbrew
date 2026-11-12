# 0008: The work feed keeps originals, encrypted, apart from their analysis

Date: 2026-10-22. Status: accepted. Issue #72.

## Context

careerbrew collected git, gh, and Jira results only. Most of the workday (Slack asks, mail, meetings, pages) never reached it. An earlier plugin prototype already ran its own feed: it kept the original events encrypted, stored the analysis apart, triaged chatter by rule, kept personal events out, and sent the operator's own actions back as evidence. Until now, connectors kept only metadata and never a message body.

## Decision

careerbrew has one core event store, the work feed (`src/careerbrew/work/feed.py`), that connectors and plugins write into.

- Each event is stored twice, apart. The original (the tool's payload, including a Slack message's text) goes to `work/feed/originals.jsonl`. The analysis (facts, triage, personal flag, the deciding rule) goes to `work/feed/events.jsonl`. Both are encrypted at rest like every data file.
- Originals are kept `feed_keep_days` days (default 30, at most 365). Then they are dropped and the analysis stays.
- Only the analysis feeds views, evidence, and the day. The original is read only to triage again (`work feed retriage`) and by `work feed open` for the owner (terminal or web app, never a piped stdout).
- Triage is by rule, not by a model: the owner's rules first (`work/feed-rules.yaml`), then built-in ones. A model-backed triage can come later on the local route.
- `personal` is one flag across sources: a private Outlook or ICS entry, a rule, or the owner's mark (the mark wins). A personal event never becomes evidence, never enters the day log, and never reaches the PM update.
- Plugins write into this store with `careerbrew work feed add --plugin <name>` instead of keeping their own; `plugin remove --purge-data` deletes their events and originals.

## Consequences

- Message text now sits on disk (encrypted, for a bounded time). The key is the backup key, as in [0007-encryption-at-rest](0007-encryption-at-rest.md).
- A rule change can be applied to the last `feed_keep_days` days only.
- Slack reads use search (`search:read`, the scope the Slack connector already has), so careerbrew sees DMs, mentions, and your own messages, not every channel.
- Tests: `tests/unit/test_issue_72.py`, `tests/e2e/us-61-work-feed.sh`.
