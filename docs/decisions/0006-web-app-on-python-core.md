# 6. The web app is TypeScript on the Python core

- **Status**: accepted
- **Date**: 2026-10-18

## Context

The owner wants careerbrew to be a web app first, with several views for planning ahead, and set a rule: UIs with more than one view use a language suited to complex UI, such as TypeScript. The core (job scan, strategy, tutor, promotion, privacy guards, watcher) is Python with about 170 tests and many leak tests.

## Decision

The owner chose: a TypeScript web UI (React + Vite, hash routing) in `web/`, talking to a small local JSON API (`careerbrew serve`) that reuses the Python core. The API binds to 127.0.0.1, needs a per-run token, checks the Host header, and blocks job-hunt views on corporate machines.

## Consequences

One extra language and toolchain (Node 20+). Every privacy guarantee stays in one tested place. A future move of core modules to TypeScript can happen behind the same API, one module at a time.
