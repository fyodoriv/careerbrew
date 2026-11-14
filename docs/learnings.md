# Learnings

What did past changes teach us? Read the learnings before you plan a similar change.

Each shipped change that had a prediction (the task's Hypothesis) has its own file in
`learnings/`, next to this page (the public release leaves that folder out; a fork starts its own). The log is append-only: to re-check a learning after a week,
add a new file instead of editing the old one.

## Read them

- All of them as one table, newest first: `uv run python scripts/learnings-table.py`
- By topic: `rg -il <topic>` in that folder

## Add one

Since 2026-11-13 a pull request puts its lesson in its body, under Predicted / Observed / Lesson, and adds
no file here: the public tree leaves the `learnings/` folder out, and the `public-readiness` check fails a
pull request that adds one ([ci.md](ci.md)). The steps below are for a private fork that keeps its own journal.

1. Make one new file in that folder: `<YYYY-MM-DD>-<slug>.md` (the date the change ships; the slug is a few
   lower-case words joined by `-`, for example the task ID or the issue number).
2. Use this shape. Line breaks inside a section are fine; the table joins them.

   ```markdown
   # <what changed, with the task ID or issue number>

   ## Predicted

   <the Hypothesis as a number: before → expected after>

   ## Observed

   <what you measured, and the test or command that shows it>

   ## Lesson

   <what to do differently next time>
   ```

3. Run `uv run python scripts/learnings-table.py --check` (the gate's `learnings` step runs it too).

Never add a table row to this page. One shared table made every open pull request conflict on GitHub
after each merge. The gate fails on a row here; `uv run python scripts/learnings-table.py --split` moves
each row into its own file.
