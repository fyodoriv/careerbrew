> Superseded in part by [0007](0007-corporate-data-stays-local.md): corporate → personal sync is removed.

# 4. Corporate and personal machines sync by encrypted files, not a server

- **Status**: accepted
- **Date**: 2026-10-18

## Context

A user may have a corporate laptop for work and a personal machine for everything else. Work evidence should reach the personal machine where policy allows. A user may want job-hunt data kept off employer hardware. A sync server would be a new place for data to leak.

## Decision

A machine role (`corporate` | `personal`) gates commands. A pairing key (Fernet, stored in a password manager) encrypts `.cbsync` bundles that the user moves by hand. The corporate plugin's `export_to_personal` (none, aggregates, redacted, full) filters what leaves the corporate machine. A corporate machine accepts settings only.

## Consequences

Sync is manual (one file per direction). A future automatic transport must keep the same filter and encryption and needs a new ADR.
