# Lessons

One dated line per run: what worked, what failed, why.

- 2026-10-19: first import. Parallel fetch subagents, one per kind of content, kept page bodies out of the main chat. A Notion SQL quota can run out mid-run; a database view query counts rows without SQL.
- 2026-10-19: cloud session. Mapped the page tree by title only (no page bodies fetched): every database maps to an existing kind. Gaps were in the web app, not the import: stories and job notes were not readable, fixed by adding Library sections. Added the cloud-session stop rule.
