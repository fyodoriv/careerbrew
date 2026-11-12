#!/usr/bin/env bash
# US-102: the work day starts with one refresh (fast-forward only), a shell preflight, and one sign-in probe (issue #260).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-28T09:00+00:00"
# Temp git repos only, with no global hooks or settings; the sign-in probe is a fake (`false`, then `true`).
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=Synthetic GIT_AUTHOR_EMAIL=synthetic@example.com GIT_COMMITTER_NAME=Synthetic GIT_COMMITTER_EMAIL=synthetic@example.com
g() { git -C "$1" "${@:2}" >/dev/null 2>&1; }
git init --quiet --bare -b main "$SCRATCH/app.git"
for c in app dirty; do git clone --quiet "$SCRATCH/app.git" "$SCRATCH/$c" 2>/dev/null; g "$SCRATCH/$c" checkout -b main; done
echo one >"$SCRATCH/app/README.md"
g "$SCRATCH/app" add README.md
g "$SCRATCH/app" commit -m first
g "$SCRATCH/app" push -u origin main
g "$SCRATCH/dirty" pull origin main
g "$SCRATCH/dirty" branch --set-upstream-to=origin/main
git clone --quiet "$SCRATCH/app.git" "$SCRATCH/pusher" 2>/dev/null
echo two >"$SCRATCH/pusher/new.txt"
g "$SCRATCH/pusher" add new.txt
g "$SCRATCH/pusher" commit -m second
g "$SCRATCH/pusher" push origin main
remote_head="$(git -C "$SCRATCH/pusher" rev-parse --short=12 HEAD)"
echo "my unsaved edit" >"$SCRATCH/dirty/README.md"
dirty_head="$(git -C "$SCRATCH/dirty" rev-parse HEAD)"

cb init >/dev/null
mkdir -p "$SCRATCH/plugin-start"
cat >"$SCRATCH/plugin-start/careerbrew-plugin.yaml" <<'YAML'
name: start-co
version: 1.0.0
work_steps:
  - {step: /synthetic-review, uses: ["browser:sso"]}
  - {step: /synthetic-notes, scope: notes}
YAML
cb plugin add "$SCRATCH/plugin-start" >/dev/null
cb config set work_checkouts "$SCRATCH/app, $SCRATCH/dirty" >/dev/null
cb config set work_signin_probe false >/dev/null

plan="$(cb day run --json)"
first="$(python3 -c 'import json,sys; print(next(iter(json.load(sys.stdin))))' <<<"$plan")"
expect "$first" "start_of_day" "the plan JSON starts with the refresh and preflight"
expect "$plan" '"command": "careerbrew day start"' "the plan names the start command"
sed_warning="$(python3 -c 'import json,sys; rows=json.load(sys.stdin)["start_of_day"]["preflight"]; r=next(x for x in rows if x["check"] == "sed"); print(r["ok"], r["status"])' <<<"$plan")"
expect "$sed_warning" "True warn" "nonblocking shell advice is a warning"
expect "$plan" '"source": {"path": "' "each step lists its resolved source"

out="$(cb day start)"
expect "$out" "fast-forwarded 1 commit(s)" "a behind checkout is fast-forwarded"
expect "$(git -C "$SCRATCH/app" rev-parse --short=12 HEAD)" "$remote_head" "the checkout is at the remote's head before stage 1"
expect "$out" "not touched: local changes" "error case: a checkout with local changes is reported"
expect "$(cat "$SCRATCH/dirty/README.md")" "my unsaved edit" "the local edit is kept"
expect "$(git -C "$SCRATCH/dirty" rev-parse HEAD)" "$dirty_head" "the checkout with local changes did not move"
expect "$out" "(same remote as " "a second folder of one remote is not watched"
expect "$out" "shell-traps.md" "every step reads the shell traps reference"
expect "$out" "sign-in probe failed" "error case: a failed sign-in probe is reported"
expect "$out" "work day start: 2 checkout(s), 1 fast-forwarded, 1 not touched" "one summary line"
expect "$out" "1 question(s)" "one sign-in question for the day"
event_seen workday.start

plan="$(cb day run --json)"
stages="$(python3 -c 'import json,sys; p=json.load(sys.stdin); s={x["step"]: x["stage"] for x in p["steps"]}; print(len(p["start_of_day"]["questions"]), s["/synthetic-review"] > s["/synthetic-notes"])' <<<"$plan")"
expect "$stages" "1 True" "one question record, and the sign-in step runs after the others"
expect "$plan" '"status": "fast-forwarded"' "the plan shows today's refresh"
expect "$(cb day render && cb report latest --run work-day)" "next 1. Sign in once for today's sign-in steps" "the page asks the sign-in question first"

cb config set work_signin_probe true >/dev/null
expect "$(cb day start)" "sign-in ok; 0 question(s)" "once signed in, no question"
expect "$(cb config set work_checkouts relative/path 2>&1 || true)" "full path" "error case: a relative checkout is refused"
printf 'name: start-py\nversion: 1.0.0\nwork_steps: [{step: /synthetic-tests, verify: "python3 -m unittest"}]\n' >"$SCRATCH/plugin-py.yaml"
mkdir -p "$SCRATCH/plugin-py" && mv "$SCRATCH/plugin-py.yaml" "$SCRATCH/plugin-py/careerbrew-plugin.yaml"
expect "$(cb plugin add "$SCRATCH/plugin-py" 2>&1 || true)" "names its interpreter" "error case: a test step with bare python3 is refused"
# Self-update runs the real CLI against a scratch checkout, never the owner launcher.
mkdir -p "$SCRATCH/app/scripts" "$SCRATCH/app/commands" "$SCRATCH/update-bin"
cp "$ROOT/scripts/install-launcher.sh" "$SCRATCH/app/scripts/install-launcher.sh"
printf 'Current work instructions\n' >"$SCRATCH/app/commands/work.md"
printf 'commands: [./commands]\n' >"$SCRATCH/app/Agentfile.yaml"
g "$SCRATCH/app" add scripts commands Agentfile.yaml
g "$SCRATCH/app" commit -m update-fixture
g "$SCRATCH/app" push origin main
cat >"$SCRATCH/update-bin/uv" <<'SH'
#!/bin/sh
printf '%s\n' "$*" >>"$UPDATE_LOG"
SH
cat >"$SCRATCH/update-bin/agentbrew" <<'SH'
#!/bin/sh
printf 'agentbrew %s\n' "$*" >>"$UPDATE_LOG"
SH
chmod +x "$SCRATCH/update-bin/uv" "$SCRATCH/update-bin/agentbrew"
cat >"$SCRATCH/start-cli.py" <<'PYTHON'
import sys
from pathlib import Path
from careerbrew import cli, paths
paths.REPO_ROOT = Path(sys.argv[1])
raise SystemExit(cli.main(sys.argv[2:]))
PYTHON
cb config set work_checkouts "$SCRATCH/app" >/dev/null
export UPDATE_LOG="$SCRATCH/update.log"
mkdir -p "$CAREERBREW_CLIENT_HOME/.claude"
out="$(HOME="$CAREERBREW_CLIENT_HOME" CAREERBREW_BIN_DIR="$SCRATCH/launcher" PATH="$SCRATCH/update-bin:$PATH" "$ROOT/.venv/bin/python" "$SCRATCH/start-cli.py" "$SCRATCH/app" day start --update-only --json)"
expect "$out" '"ok": true' "self-update repairs run before any day plan"
expect "$(cat "$UPDATE_LOG")" 'sync' "dependency sync is called"
expect "$(cat "$UPDATE_LOG")" 'run careerbrew plugin refresh' "installed plugins refresh in a fresh process"
expect "$(cat "$UPDATE_LOG")" 'agentbrew sync' "configured agent commands sync"
expect "$(cat "$SCRATCH/launcher/careerbrew")" '# careerbrew launcher' "a missing launcher is repaired"
# A local edit refuses update before any repair command can run.
echo edited >>"$SCRATCH/app/commands/work.md"
before_lines="$(wc -l <"$UPDATE_LOG")"
set +e
out="$(HOME="$CAREERBREW_CLIENT_HOME" CAREERBREW_BIN_DIR="$SCRATCH/launcher" PATH="$SCRATCH/update-bin:$PATH" "$ROOT/.venv/bin/python" "$SCRATCH/start-cli.py" "$SCRATCH/app" day start --update-only --json)"
update_rc=$?
set -e
[ "$update_rc" = 1 ] || fail "dirty self-update must exit 1"
expect "$out" '"status": "local changes"' "dirty checkout is reported"
[ "$(wc -l <"$UPDATE_LOG")" = "$before_lines" ] || fail "a refused update must not install local edits"
expect "$(cat "$SCRATCH/app/commands/work.md")" edited "local edit is preserved"

# Owner-approved GitHub header repair: scratch settings only, no live credentials.
mkdir -p "$SCRATCH/repair-bin"
printf '#!/bin/sh\nexit 0\n' >"$SCRATCH/repair-bin/launchctl"
printf '#!/bin/sh\nprintf "%%s\\n" "synthetic-header-canary-7"\n' >"$SCRATCH/repair-bin/gh"
chmod +x "$SCRATCH/repair-bin/launchctl" "$SCRATCH/repair-bin/gh"
export PATH="$SCRATCH/repair-bin:$PATH"
export CAREERBREW_CLAUDE_CONFIG="$SCRATCH/claude.json"
uv run python - "$CAREERBREW_CLAUDE_CONFIG" <<'MCP_SETTINGS'
import json, sys
from pathlib import Path
link = Path(sys.argv[1])
target = link.with_name("real-settings.json")
target.write_text(json.dumps({"keep": True, "mcpServers": {"fixture": {"type": "http", "url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}", "X-Test": "keep"}}}}))
target.chmod(0o640)
link.symlink_to(target)
MCP_SETTINGS
set +e
out="$(cb day start --fix 2>&1)"; repair_rc=$?
set -e
[ "$repair_rc" = 2 ] || fail "an unapproved configuration repair is refused"
expect "$out" "--yes" "the repair requires explicit owner approval"
expect "$(cb day start --fix --yes --json)" '"changed": 1' "an approved repair writes the helper"
expect "$(cb day start --fix --yes --json)" '"changed": 0' "the second repair changes nothing"
uv run python - "$CAREERBREW_CLAUDE_CONFIG" <<'MCP_PROOF'
import json, stat, subprocess, sys
from pathlib import Path
link = Path(sys.argv[1])
assert link.is_symlink()
assert stat.S_IMODE(link.resolve().stat().st_mode) == 0o640
data = json.loads(link.read_text())
assert data["keep"] and data["mcpServers"]["fixture"]["headers"] == {"X-Test": "keep"}
assert "synthetic-header-canary-7" not in link.read_text()
helper = data["mcpServers"]["fixture"]["headersHelper"]
result = subprocess.run(helper, shell=True, capture_output=True, text=True, check=True)
assert json.loads(result.stdout) == {"Authorization": "Bearer synthetic-header-canary-7"}
MCP_PROOF
browser_story
(cd "$ROOT/web" && env -u CAREERBREW_HOME -u CAREERBREW_NOW npx playwright test e2e/mcp-repair.spec.ts)
echo "ok US-102 work day start"
