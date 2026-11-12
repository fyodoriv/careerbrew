# Hallmark audit of web/: after

The Hallmark audit of `web/` after the restyle (branch `feat/hallmark-design`), 2026-10-27. Task `hallmark-design`. The before audit is [`hallmark-audit-before.md`](hallmark-audit-before.md).

**How it ran.** The same command as before, with `--kept` in place of `--before`, on the seeded e2e server, for the same eight views. The render tier waited for `networkidle2` (the app's live poll never reaches `networkidle0`).

**Result.** 26 Floor (FAIL) and 922 Reflex findings before; 0 Floor and 0 open Reflex after. The kept check finds every link (918/918), field (8/8), handler (115/115), test hook (1/1) and heading (85/85). One figure of 40 is dropped on the record: the seed's wall-clock time.

**What changed.** The look only: the Furrow theme (warm manila paper, ink text, one tangerine fill with ink on it), one 4 px spacing scale, one type scale on the system font, three weights, `:active` and three-channel disabled states, field edges at 3:1, no side stripes, no tracked caps. The next step is a raised notebook card. Layout, copy, controls and test ids stay. The meta strings that joined parts with a middle dot now use commas or semicolons (gate 63).

**Kept findings, each with its reason** (the waiver lines at the top of `web/src/styles.css`):

| Kind | Gate | Where | Why it stays |
|---|---|---|---|
| waive | 49b | posting titles, Library question links, source links on a phone | the owner's own titles; they wrap so the page never scrolls sideways (issue #67) |
| waive | 24b | `min(36%, 150px)` on the stacked table cell | the cell's padding is the width of its column label, a share of the row, not a spacing step |
| waive | 38iii | `.stepper h3 .tag` | a muted tag naming the step's kind as data, not an accented word in a head |
| waive | 72 | `li.block` on Today and Week | the check measures text inside a closed Details fold, which is not painted until it opens |
| dismissed | 3 | the Library reader's blockquote rule | it marks quoted prose; it is not a card stripe |
| dismissed | 70 | "Sent to", "careerbrew work done" | a real server answer and a command hint, not a success state |
| answered | F1 | straight quotes | they are in code and typed values |
| reworded | figure | "19:55" | the PM draft's time is the seed's wall clock, new on each seed |

**Finish review (fresh context).** Verdict `fix`, 7 findings, all acted on: links in tables and card heads get the underline again; meetings are solid boxes, not dashed like empty days; the next step and the study card get a lighter surface in dark mode; the dark task fill is calmer; disabled buttons keep an edge in dark mode; the page head is 16 px shorter and its rule is `--line-strong`, not ink.

**Pictures.** Every view's before and after, light, desktop and phone: `web/visual/__screenshots__/us-35-web-app.spec.ts/view-<view>-<desktop|phone>-darwin.png` (before: the same path on `main`).


## Counts

| Grade | Count |
|---|---|
| WAIVED | 908 |
| DISMISSED | 3 |
| ANSWERED | 2 |
| NOTE | 5 |

| Grade | Gate | Count |
|---|---|---|
| ANSWERED | F1 | 2 |
| DISMISSED | 3 | 1 |
| DISMISSED | 70 | 2 |
| NOTE | 19 | 4 |
| NOTE | waive | 1 |
| WAIVED | 24b | 1 |
| WAIVED | 38iii | 1 |
| WAIVED | 49b | 904 |
| WAIVED | 72 | 2 |

## Findings

- `WAIVED 24b` :213 padding: var(--space-1) var(--space-… (off the 4px scale) on .table-wrap td, .table-wrap td.num
- `WAIVED 38iii` :274 accented word inside a heading (.stepper h3 .tag)
- `DISMISSED 3` :323 side stripe border-left: 3px solid var(--line-strong) on .prose blockquote
- `NOTE waive` :1 4 reflex gates waived (24b, 38iii, 49b, 72): that is a house style, not… | fix: Lock it into a design.md so later pages inherit it instead of re-arguing it
- `WAIVED 72` :1 [1440px] text crowds the edge of its box: li.block bottom -11px; li.block bottom -11px
- `WAIVED 72` :1 [1440px] text crowds the edge of its box: li.block bottom -11px; li.block bottom -11px; li.block bottom -11px
- `ANSWERED F1` :111 straight quote in prose x1 (first: Set your PM's address:)
- `DISMISSED 70` :1 a script prints success ("Sent to ${r.sent.to}") with no request behind it
- `ANSWERED F1` :374 straight quote in prose x3 (first: Today's plan)
- `DISMISSED 70` :1 a script prints success ("careerbrew work done <id>") with no request behind it
- `NOTE 19` :140 dash "–" in copy | fix: A comma, a colon, a full stop or parentheses; never a dash
- `NOTE 19` :20 dash "—" in copy
- `NOTE 19` :87 dash "–" in copy
- `NOTE 19` :372 dash "–" in copy

## Gate 49b (a link label wraps), grouped

One finding per long link at 320 px; the labels are the owner's titles.

- :1: 904 links, for example: [320px] link "code.example.com/…/test_checkout" wraps to 2 lines (a)
