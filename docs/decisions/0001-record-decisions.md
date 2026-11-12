# 1. Record decisions as ADRs

- **Status**: accepted
- **Date**: 2026-10-18

## Context

Agents and future you need to know why a hard-to-undo choice was made, without
reading old chats.

## Decision

Record each such choice as `docs/decisions/NNNN-<title>.md` with Context,
Decision, and Consequences. The first ones: the stack (Python (uv, pytest, ruff, pyright)) and the
profile (assistant). The repo was scaffolded and scored with the
`new-project` skill (`scripts/check-setup.sh --profile assistant --strict`).

## Consequences

A PR that reverses a decision adds a new ADR that supersedes the old one.
