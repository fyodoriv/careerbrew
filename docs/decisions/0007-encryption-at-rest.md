# 7. Encrypt the data root at rest

- **Status**: accepted
- **Date**: 2026-10-19

## Context

The data root holds 1:1 notes about people, salaries, interview history, and promotion evidence. It used to be plaintext, protected only by folder permissions (700) and the OS disk encryption. A copied folder, a sync client, a backup tool outside careerbrew, or another program (or agent) reading the disk would see everything.

## Decision

Every file careerbrew writes under the data root is a Fernet token behind a `CBV1` header (`src/careerbrew/vault.py`), written through `store`. The key is the existing backup key, outside the data root (mode 600): one secret in the password manager opens both data and backups. Reads accept plaintext, so hand-edited and older files still work and `careerbrew data encrypt` can seal them. `logs/` (content-free) and `out/` (reports the user opens) stay plaintext. Sealed JSONL appends are read-add-rewrite under a file lock. A missing key never gets silently replaced while sealed files exist.

## Consequences

- Files cannot be edited with a plain editor any more: `careerbrew data edit <file>` opens a short-lived plaintext copy and re-encrypts it.
- Code must use `store.read_text` / `store.write_text`, never `Path.read_text`, for data-root files (tests too).
- Losing the key loses the data; `careerbrew data status` names the key path, and the setup docs tell the user to save it.
- An agent with shell access on the same machine can still read the key; this protects the data at rest, not against a process running as the user.
- `careerbrew data decrypt --yes` turns it off for a data root.
