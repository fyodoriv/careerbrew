# Contributing to careerbrew

Thank you for using careerbrew. Start with [`AGENTS.md`](AGENTS.md) for the
commands, the gate (`scripts/check.sh`), and the privacy rules
([`docs/privacy.md`](docs/privacy.md)).

## Report problems and ideas

**Fyodor, the author of careerbrew, is glad to get every report.** If something in
careerbrew did not work as expected, or could work better, please say so. A short
report from a real user is the most useful contribution there is, and it is
always welcome, even when you are not sure it is a bug.

### Your agent will offer to do it for you

If you use careerbrew with an AI coding agent (Claude Code, Codex, and others),
the agent is told to offer a report when careerbrew fails, surprises it, or could
clearly work better. This holds in every setup: a clone or a fork of this repo
(this repo's `AGENTS.md` says so), or careerbrew installed through agentbrew
(the always-loaded agent rules and the `report-upstream` skill say so).

The agent never posts anything on its own. It works in these steps:

1. It writes a short draft: what ran, what was expected, what happened, the
   version, and a suggested fix.
2. It runs `agentbrew report --title "<title>" --body-file <draft.md> --repo fyodoriv/careerbrew`.
   This posts nothing. It replaces private details with placeholders: your
   home folder path, your user name, email addresses, tokens and API keys,
   private keys, IP addresses, internal host names, and the private patterns
   of your local leak gate, if you have one. Then it re-scans the result and
   prints a checklist like this:

   ```
   Safety check (nothing has been posted):
     ✓ none found: tokens, API keys, and private keys
     ✓ 1 replaced with a placeholder: your home folder path
     ✓ none found: email addresses
     ✓ re-scan of the final text: nothing private left
   ```

3. It shows you the exact title and body, and any similar issue that already
   exists, and asks: "Post this issue to github.com/fyodoriv/careerbrew? It is public.
   (yes / edit / no)".
4. Only after your yes does it post (`agentbrew report ... --yes`). A refused
   or failed re-scan (`✗`) is never posted.
5. If the fix looks small, it asks a second question: "Want me to make a PR
   that fixes it?" Only after that second yes does it make the change, run
   the repo gate (`scripts/check.sh`), and open the PR with `agentbrew contribute`
   (it forks the repo for you).

A run with nobody at the keyboard only saves the draft
(`~/.local/state/agentbrew/reports/`) and names it in the run report.

### careerbrew-specific safety

- A report never includes anything from `private/` (your career, job, or work
  data), not even scrubbed. It describes the careerbrew command and its error
  only.
- If you cannot open issues on `fyodoriv/careerbrew`, the agent saves the
  draft and tells you where it is; send it to the author another way.

### Do it by hand

Open an issue at https://github.com/fyodoriv/careerbrew/issues/new with the same
content: what you ran, what you expected, what happened, and the version.
Please leave out secrets, tokens, and private paths.

### Turn the offers off

Run `agentbrew report --off`. Agents then stop offering reports on this
machine. `agentbrew report --on` turns them back on.