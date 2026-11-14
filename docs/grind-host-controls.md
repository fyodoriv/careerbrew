# Grind host controls

## Shared memory

When shared semantic memory is available, call get_bootstrap_profile with this
agent ID and task summary, then search relevant repository procedures and mistake
notes before work. Call commit_session_legacy at session end with reusable
source-backed decisions. Never store career records, business payloads, credentials,
or raw session transcripts in shared memory. Intentionally unconfigured memory
permits source-doc or deterministic CLI-only work with an explicit no-recall limit.
Configured memory failure is different: name the failed tool, say "shared memory
unavailable", and block context-dependent readiness/advice until repaired and
successfully retried. Never silently disable it or substitute docs for required
recall, even under pressure to continue. Report session-record failure as
unrecorded. Deterministic CLI-only actions keep data local and remain available.

## Host controls

Select controls from the tools this session actually exposes. Never infer a host
from a command's name. Do not translate tool names without checking availability.

### Codex / ChatGPT Work

For owned child workers, use list_agents and wait_agent for status, then
followup_task for an idle worker or send_message for an active one. If only native
spawn_agent/wait/send_input controls are exposed, use that equivalent owned-worker
route. Follow native completion or needs-attention states; output silence alone
does not prove a stall. Never message an unrelated chat or launch a user-owned
chat to replace an unavailable worker tool.

Preserve native permissions and approval settings; do not invent Claude sandbox flags.
Run gate/poll commands with exec_command and retain their session ID; use
write_stdin to collect progress and the final exit code. Poll the owned log and
status with bounded waits. A failed listing is an error, not an empty queue.
If native worker controls are absent, run sequential work and gate commands in
this chat. The Claude transcript helper is not a Codex status source. Do not use
Claude-only tools, arguments or paths in this branch.

### Claude Code

The Monitor, SendMessage, Bash background option and transcript helper below apply
only when those Claude controls exist. Otherwise use the sequential fallback.
The shared queue, gate, ownership and report rules apply to both hosts.


Keep every worker alive:

- A worker waits for the hosted checks of its pull request (`gh run watch <run-id> --interval 30`, [ci.md](ci.md)); it needs no local full gate. Only when the hosted checks are unavailable does a worker run the optional local gate, and then it waits for it in the foreground: it starts `(heavy-run scripts/check.sh > <log> 2>&1; echo "rc=$?" >> <log>)` in the background, then repeats the foreground Bash call `for i in $(seq 1 110); do grep -q '^rc=' <log> && break; sleep 5; done; tail -5 <log>` (timeout 600000) until the log has its `rc=` line. It never ends its turn to wait for a background completion notice: a killed shell sends none, and the worker waits forever. If the log stops growing and `heavy-run --status` does not list the command, the gate died: start it again.
- On Claude Code only, for the whole batch, keep a Monitor on `python3 -I ~/.claude/skills/run-goal/scripts/stalled-agents.py --session <id> --minutes 20 --watch 300` (agentbrew run-goal skill; `<id>` is this session's folder name under `~/.claude/projects/<project>/`), and re-arm it at each expiry. On a `STALLED <agent>` line, resume that worker at once with `SendMessage`. A `KILLED <task>` line next to it names the gate shell that died: tell that worker to rerun its gate in the foreground.
- A check-in judges each worker by its last transcript write (that script), never by who holds the `heavy-run` lock.
