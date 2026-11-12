# 0007: Corporate data stays on the corporate machine

Date: 2026-10-20. Status: accepted. Supersedes the corporate → personal part of [0004](0004-machines-and-sync.md).

## Context

ADR 0004 let a corporate machine export work evidence to the personal machine, filtered by the plugin policy `export_to_personal` (default `redacted`). Other exits could also move corporate data off the machine: backups to a cloud-synced folder, a data folder inside iCloud or OneDrive, and a hosted model route marked `private_data: true`.

The user's rule: corporate data is fully kept on the corporate machine, with nothing synced anywhere else.

## Decision

On a corporate-role machine:

- `sync export` refuses. A personal machine refuses a corporate bundle. Personal → corporate still moves `ladder` and `target_level`.
- A backup target in a cloud-synced folder is refused.
- `machine ready` and `careerbrew checkup` fail when the data folder or a backup target is cloud-synced.
- Business data goes only to a model route on this machine (localhost), whatever `private_data` says.

`export_to_personal` is ignored. New plugins write `none`.

## Consequences

- Promotion evidence from work no longer reaches the personal machine. Personal-side features that used it (LinkedIn block from work evidence) run on the corporate machine instead, and only if your employer's confidentiality policy allows public text about your work.
- The guards and their tests: `tests/unit/test_corporate_exits.py`, US-15.
- Out of scope: company-managed Time Machine or MDM backups.
