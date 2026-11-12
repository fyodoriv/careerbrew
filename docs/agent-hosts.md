# Agent hosts

Careerbrew owns career data and workflow sources. Agentbrew deploys those sources
to Claude Code and ChatGPT Work/Codex. CLI actions and the web Actions view use
the existing parser-generated action catalogue; no extra action list is maintained.
Use `careerbrew actions --json` for arguments and terminal-only reasons.

Work steps resolve the owning checkout's declared command files and skill
directories before host mirrors. The lookup keeps exact-file selection and
checks ownership again before reading. A plugin step stays within its declared
plugin source. A Codex legacy `prompts/*.md` file can supply a resource; this does
not make the legacy prompts picker available in the native GUI.
Legacy names such as `/team:review` can read a nested resource under a selected
source directory. They do not publish that nested file as a native entrypoint.
An exact-file declaration admits only that selected file, never its siblings.
Resource names must stay relative to that selection. Empty or absolute names
and `.`/`..` path parts return a source error before any file is read.
Implicit nested resources stay within the selected directory after symlinks are
resolved. Explicitly selected source aliases keep their existing checkout scope.

## Session memory and privacy

When semantic memory is available, call `get_bootstrap_profile` with this agent ID
and task summary, then use ranked memory search for relevant repository procedures and search mistake notes
before consequential work. At session end call `commit_session_legacy` once with
source-backed reusable decisions, errors/resolutions and corrections. Do not put
career records, business payloads, credentials or raw transcripts into shared
memory. On a corporate machine, use only a shared memory store that the employer
approved.

If shared memory is intentionally unconfigured, state that recall is not configured
and use source docs or deterministic CLI-only work without claiming memory parity.
If configured shared memory fails, name the failed tool and say "shared memory
unavailable". A failed get_bootstrap_profile or ranked search blocks
context-dependent readiness and remembered-context advice until repair and a
successful retry. A failed session-record tool must be reported as unrecorded;
never claim recording succeeded. Offer repair or deterministic CLI-only work.
Do not silently disable configured memory, relabel failure as unconfigured, or use
source docs as a substitute for required recall, even when asked to keep going.
Local deterministic CLI actions still work. Native account memory/privacy settings
are separate and are never enabled by deployment.

Private records remain owned by Careerbrew. Use CLI aggregates/ids and its encrypted
store under the existing plugin policy. Agent-facing output never gains owner-UI
privilege merely because a different host runs it. Never publish personal reports.

## Worker and shell controls

`careerbrew-grind` selects exposed capabilities. Codex owned-child collaboration
status/wait/message tools and shell-session polling replace Claude Monitor,
SendMessage and Bash background options. Do not message unrelated user chats.
Without worker tools, execute sequentially in the current chat. A quiet worker is
not proved stalled; check native state, the owned gate log and final exit status.
Claude transcript monitoring applies only on Claude. Poll/list failures are errors,
not an empty queue. Use bounded waits and keep the foreground gate alive.

## Command names shadowed by skills

A host can choose an installed skill over a command with the same name. For
example, a skill named `work` can answer `/work` instead of careerbrew's command.
The deployed-command doctor checks the command file; it does not detect this
collision. From the careerbrew repo root, list direct matches in the user and
project skill folders below without reading their contents:

```sh
for command in commands/*.md; do
  name=${command##*/}; name=${name%.md}
  for root in "$HOME/.agents/skills" "$HOME/.claude/skills" "$HOME/.codex/skills" \
              .agents/skills .claude/skills .codex/skills; do
    if [ -f "$root/$name/SKILL.md" ]; then printf '%s: %s\n' "$name" "$root/$name"; fi
  done
done
```

This check does not cover nested, plugin-provided, or aliased skills. Check those
and the project skill folders of the directory where you invoke the command if
it differs from this repo. Rename the conflicting skill in its source,
sync it through its owner, and start a new session. Then invoke `/work` again and
check that careerbrew's work-day runner starts.

## Source deployment

Repository commands come from `Agentfile.yaml`. Installed private plugins provide
canonical selected command files, declared by Careerbrew's private `agentbrew/Agentfile.yaml`
under its data root. A normal `agentbrew sync --only commands` runs with that private
folder as cwd: its source ownership prunes only removed file declarations belonging to
that fragment and preserves other owners. Agentbrew alone performs host transforms
and emits native command-skill wrappers. Never register generated agent mirrors.
No private plugin content belongs in the public repo or a global public catalog.

Legacy plugin-owned Claude copies migrate only if the prior canonical source is
available and byte-identical. The encrypted private command archive is read back
and verified before leaf removal. Owner edits/untracked copies remain unchanged and
stop migration for attention. The private archive receipt records a restore path;
restore through the source workflow, never overwrite an edited target.

The deployed-command doctor checks native source-reference bytes, explicit-only
invocation policy and wrapper safeguards, as well as transformed legacy prompts.
An edited target is reported for source adoption; it is not force-overwritten.
Work steps resolve repository, shared `.agents`, native `.codex`, then `.claude`
sources in that order for non-plugin steps. A plugin-owned step resolves only the
declared plugin's canonical resource. A missing resource returns `source_error`;
mark that step failed and repair its source, without using another owner's copy.
Private plugin skills remain work-step resources, not globally published `$skill`
catalogue entries. Published repository skills use `careerbrew-*` names and
`skillDirectories` for direct discovery on both primary hosts.

## Feed and connector adapters

Feed transcript ingestion normalizes supported Claude and native call/result rows
before existing source mapping, time filtering and deduplication. Only same-folder
saved-output pointers may be followed. Unsupported input fails explicitly; quiet
empty results and tool errors remain distinct. Use `work feed add` for a supported
host-independent JSON event route. Tests use synthetic payloads only.

MCP preflight returns a sanitized host-scoped failure for present unreadable or
malformed config, including enabled server entries, boolean enabled flags,
header maps and invalid URL parsing. Explicitly disabled entries are skipped
before their payload is checked. Absent optional host config is allowed;
permissive server discovery retains its existing behavior. It labels Claude
and Codex config scopes. It checks local wiring, not successful OAuth or account access. No auth
value is printed. The CLI and GUI can have different effective permissions.

The existing doctor/checkup also reports configured CLI availability and local
credential status. See [the install playbook](../INSTALL.md#5-check-and-fix) for
bounded probes, optional-client behavior and repair steps. A signed-in CLI does
not prove service access or replace required shared-memory recall.

## Future additions and verification

Every story declares `dual-host-shared-memory`; every skill and command carries
the matching source contract marker and a runtime route via careerbrew where to this guide. The existing story
checker rejects omissions, including newly added workflow files. Existing action
parity tests retain parser/catalog/UI equality, argument validation, confirmation
and terminal-only reasons. Native adapters have paired synthetic fixtures and
owner-file, privacy and failure-path tests. Run `CHECK_EVALS=1 scripts/check.sh`
through `heavy-run` before delivery. Available catalogues are not a claim that every
real career workflow has been executed; account authorization stays separate.

The checker and required-entrypoint doctor share one inventory from this
checkout's `Agentfile.yaml`: `commands` selects folders or individual Markdown
files; `skillDirectories` selects published skill collections. Custom folders and
nested command paths receive the same contract and health checks as the default
folders. A selected file never includes its unselected siblings. Each source keeps
its declared command or skill role; a file declared both ways must pass both.
Story `Skill` references name their declared repository-relative skill folder.

Both a declared path and its current target must stay inside the owning checkout.
An outside alias is rejected even if it points back inside; use the checkout's
path instead. Internal command aliases retain their selected filename. The current
selection is checked before body reads, so an external or retargeted pointer
fails closed. Empty/comment-only Agentfiles retain the legacy-folder fallback;
malformed nonmapping YAML still fails. Private plugin resources keep their separate canonical plugin
route; this inventory never scans them. Deployment state stays owned by agentbrew.

Native transcript fixtures follow the installed Codex 0.166 schema and
[OpenAI's versioned protocol](https://github.com/openai/codex/blob/rust-v0.166.0/codex-rs/protocol/src/protocol.rs):
ResponseItem function calls/output (including input_text content) and event_msg
MCP completion records. They do not depend on a live account transcript.

Careerbrew repository command basenames use the generic bridge's direct-name
subset: lower-case letters/digits with single hyphens, no reserved `x-` prefix,
and at most 64 characters including `agentbrew-command-`. The story gate rejects
new names outside that subset before the own-repo doctor could misidentify them.
Private plugin names are handled by agentbrew provenance and its generic adapter.

## Host primitive routes

The story gate checks host-specific calls against this table. A missing required
capability blocks that action with its tool name. Never claim it ran, read a
private browser profile, grant OAuth, or bypass a permission to continue.

| Primitive | Portable route |
|---|---|
| `AskUserQuestion` | Use the host's question tool when available, else ask inline. |
| `TodoWrite` | Use the host plan tool or CLI status rows. |
| `Task` | Use owned native workers when available; otherwise run sequentially. |
| `Agent` | Use owned native workers when available; otherwise run sequentially. |
| `Skill` | Read the canonical prefixed SKILL.md from the source checkout. |
| `Monitor` | Use native owned-worker status/wait or poll the owned CLI session. |
| `SendMessage` | Message only owned workers; otherwise return results sequentially. |
| `run_in_background` | Use an owned execution session and poll it; otherwise run sequentially. |
| `get_page_text` | Use the available browser snapshot/text tool or agent-browser. If none is available, block browser research. |
| `~/.claude/` | Resolve sources with careerbrew where or agentbrew provenance, never hard-code an agent mirror. |

Browser research is optional only when the requested action does not require it.
A canonical private plugin resource has the same tool checks on both hosts. Its
presence proves source resolution, not successful private service authorization.

## Skill section order

Load `careerbrew-best-practices` before a skill's first step. Keep its host
contract before `## Iterate`, which remains the final section. The evaluator
checks the published skill name and preserves that section-order rule.
