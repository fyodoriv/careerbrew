# Memory index (example shape)

The live index is `private/memory/MEMORY.md`. One line per durable fact, newest
first, each pointing to a topic file next to it:

- [Topic title](topic-file.md) — one-line hook (YYYY-MM-DD)

Keep the index short; it loads at the start of every session. Update an entry
instead of adding a duplicate. Delete entries that turn out wrong.
