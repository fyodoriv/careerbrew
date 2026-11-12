# 3. Treat every output path as a data exit

- **Status**: accepted
- **Date**: 2026-10-18

## Context

The user's data is personal (job hunt) and business (employer work). An agent's stdout goes to its model provider, so the terminal is an exit too, alongside git, logs, backups, hosted models, and machine sync.

## Decision

Each exit has one guard and one canary test (`docs/privacy.md`). Stdout is redacted and business content needs `--show` on a real terminal or `agent_may_read_business: true`. Hosted models see only `private_data: true` routes for non-public data. Job-hunt data never goes to a corporate-role machine.

## Consequences

New features that print, log, store, or send data must add a canary test. Some agent help in work mode is limited unless the company approves its AI provider.
