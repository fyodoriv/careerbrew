# 2. Corporate plugins: declarative, hashed, multi-plugin, strictest policy

- **Status**: accepted
- **Date**: 2026-10-18

## Context

Company content (internal ladders, sources, terms, data rules) must never live in this repo. agentbrew team overlays solve the routing but allow one team at a time and trust the overlay fully.

## Decision

A plugin is a folder or git repo with `careerbrew-plugin.yaml`. It installs into `private/plugins/<name>/` with a SHA-256 tree hash in `private/plugins.lock.json`. Install never runs code; a `command` source needs `--allow-exec`. Several plugins can be active; resource ids must not clash. Policies merge to the strictest value. Every record carries `origin`, so removal can purge data.

## Consequences

A plugin author writes YAML, not code. A new policy field needs a merge rule in `plugins.effective_policy` and a test.
