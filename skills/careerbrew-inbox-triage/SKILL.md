---
name: careerbrew-inbox-triage
description: Scans the user's Outlook, Fastmail, and Proton (Bridge) mail metadata for LinkedIn job alerts, application confirmations, and recruiter or interview emails, then proposes the follow-up. It reads only sender, subject, and date; it never reads bodies and never sends mail. Use when the user says "check my job emails", "did any recruiter write", "scan my inbox", or "connect my email". Don't use to write or send replies, or on a corporate-role machine for personal accounts.
---

# Inbox triage

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. If `private/mail.yaml` has no accounts, help set them up from `profile/examples/mail.yaml`:
   - Outlook: register a public client app, then store the token with `careerbrew secret set careerbrew.graph` (scope Mail.ReadBasic).
   - Fastmail: create a read-only API token (Settings → Privacy & Security), then `careerbrew secret set careerbrew.fastmail`.
   - Proton: run Proton Mail Bridge (paid plan), then store the Bridge password with `careerbrew secret set careerbrew.proton-bridge`.
2. `uv run careerbrew mail scan --days 7`. Report the counts per group.
3. For each interview email: ask the user which company and stage, then `careerbrew jobs log <company> <tier> <stage>`.
4. Draft replies only when asked; the user sends them.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-inbox-triage/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
