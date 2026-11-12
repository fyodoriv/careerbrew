# Method notes

Each rule names where it comes from. Re-check a source before you change a rule.

## Layers

- Zoom like the C4 model: the context map (level 1) shows who and what talks to the project; containers (level 2) show each part that deploys on its own. A widget is not a container, so each container names how it deploys. Source: https://lucid.co/blog/c4-model
- The codemap answers "where is the code that does X" with names, not links, so it stays true after files move. Source: https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html
- Invariants and boundaries say what must never happen; they are the most useful part of an architecture doc. Source: the same matklad post.
- "Why it is this way" is explanation mode: it gives reasons and trade-offs, not steps. Source: https://www.diataxis.fr/map/
- Start with what, why, status; then read next and open questions with who can answer. Source: https://learn.chatgpt.com/codex/use-cases/codebase-onboarding

## Load

- At most 4 new terms per section, each defined at first use; the glossary repeats definitions, never replaces them. Experts can skip ahead (expertise reversal). Source: https://www.computingatschool.org.uk/forum-news-blogs/2022/june/cognitive-load-and-a-level-programming/
- "Check yourself" uses retrieval practice: 3 questions with hidden answers. Source: the same post.

## Evidence

- Every bullet and table row cites a source inline as `[S<n>]`. A primary source is the thing itself (code at a sha, the ticket, the decision doc); a lead is a pointer (a chat message, a summary). No claim rests on leads alone.
- Each source has a locator (URL, ticket key, doc id, repo@sha, or path) and a read date or sha. Unused sources are removed.
- "Considering" in a source means proposed, not decided.
- Quotes match the source word for word, and an absence claim names the check that proved it. Only the grader catches these.

## Skill shape

- Frontmatter with name and description, a body under 500 lines, and references one level deep. Source: https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices
