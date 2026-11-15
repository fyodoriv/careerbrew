#!/usr/bin/env bash
# US-89: anonymous usage counts leave only after a yes, never with content; a fake local endpoint only.
source "$(dirname "$0")/lib.sh"
unset CI DO_NOT_TRACK CAREERBREW_TELEMETRY  # this story tests the off switches itself; a hosted runner sets CI
cb init >/dev/null
# Error case: no answer yet, so nothing is sent, even with a key.
fake="syntheticE2Ekey"  # a made-up value of the key shape, never a real key
export CAREERBREW_POSTHOG_KEY="phc_$fake"
expect "$(cb telemetry status)" "telemetry: off (not answered yet" "error case: no answer, nothing is sent"
expect "$(cb telemetry status)" "consent: unset" "the answer starts unset"
expect "$(cb telemetry send)" "nothing sent" "error case: send before a yes sends nothing"
# Setup saves its numbers locally; no event is written before the yes.
started=$(($(date +%s) * 1000 - 4000))
expect "$(cb telemetry record-install --started "$started" --ok true)" "saved locally" "setup saves its install numbers"
refute "$(cat "$CAREERBREW_HOME/logs/events.jsonl" 2>/dev/null || true)" "careerbrew.install" "setup alone writes no event line"
# A fake endpoint that records each request body; nothing real is ever sent.
PORT_FILE="$SCRATCH/port" BODIES="$SCRATCH/bodies.jsonl"
"$ROOT/.venv/bin/python" -I - "$PORT_FILE" "$BODIES" <<'PY' &
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
port_file, out = sys.argv[1], sys.argv[2]
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        with open(out, "ab") as fh:
            fh.write(self.rfile.read(int(self.headers["Content-Length"])) + b"\n")
        self.send_response(200); self.end_headers()
    def log_message(self, *a): pass
srv = HTTPServer(("127.0.0.1", 0), H)
open(port_file, "w").write(str(srv.server_address[1]))
srv.serve_forever()
PY
FAKE=$!
trap 'kill $FAKE 2>/dev/null; rm -rf "$SCRATCH"' EXIT
for _ in $(seq 50); do [ -s "$PORT_FILE" ] && break; sleep 0.1; done
export CAREERBREW_TELEMETRY_URL="http://127.0.0.1:$(cat "$PORT_FILE")"
# Business text from before the yes: it must never leave.
cb work task "Prepare $CANARY_EMPLOYER offer for $CANARY_PERSON" --hours 1 >/dev/null
expect "$(cb telemetry on)" "telemetry on" "telemetry on saves yes"
expect "$(cb telemetry status)" "telemetry: on (on)" "a yes and a key turn it on"
dry="$(cb telemetry send --dry-run)"
expect "$dry" '"api_key": "<posthog_key>"' "the dry run hides the key"
expect "$dry" '"event": "careerbrew.install"' "the yes emits the saved install numbers"
refute "$dry" '"command": "work.task"' "nothing from before the yes is pending"
refute "$dry" "phc_$fake" "privacy: the dry run never prints the key"
installs="$(cb telemetry send --dry-run --json | "$ROOT/.venv/bin/python" -c 'import json,sys; print(sum(e["event"]=="careerbrew.install" for e in json.load(sys.stdin)["body"]["batch"]))')"
[ "$installs" = 1 ] || fail "one install event after one setup and a yes (got $installs)"
[ ! -s "$BODIES" ] || fail "a dry run sends nothing"
cb work task "Second $CANARY_EMPLOYER task" --hours 1 >/dev/null
expect "$(cb telemetry send)" "sent " "send posts the batch"
sent="$(cat "$BODIES")"
expect "$sent" '"distinct_id"' "events carry the random install id"
expect "$sent" '"command": "work.task"' "a command after the yes is sent"
for leak in "$CANARY_EMPLOYER" "$CANARY_PERSON" "$CAREERBREW_HOME" "$(hostname)"; do refute "$sent" "$leak" "privacy: no content, path, or host leaves"; done
# A second setup of the same data folder emits no second install event.
cb telemetry record-install --started "$started" --ok true >/dev/null
expect "$(cb telemetry send)" "sent " "a second send works"
[ "$(grep -o '"careerbrew.install"' "$BODIES" | wc -l | tr -d ' ')" = 1 ] || fail "one install event per data folder"
[ "$(grep -c '"work.task"' "$BODIES")" = 1 ] || fail "nothing is sent twice"
# Off switches.
n="$(wc -l <"$BODIES")"
expect "$(DO_NOT_TRACK=1 cb telemetry send)" "DO_NOT_TRACK is set" "DO_NOT_TRACK stops it"
expect "$(CI=true cb telemetry send)" "CI is set" "CI stops it"
expect "$(CAREERBREW_TELEMETRY=0 cb telemetry send)" "CAREERBREW_TELEMETRY=0" "CAREERBREW_TELEMETRY=0 stops it"
expect "$(cb telemetry off)" "telemetry off" "telemetry off"
expect "$(cb telemetry send)" "you said no" "off sends nothing"
[ "$(wc -l <"$BODIES")" = "$n" ] || fail "no request while off"
# A dead endpoint never fails another command.
cb telemetry on >/dev/null
CAREERBREW_TELEMETRY_URL="http://127.0.0.1:9" cb day watch once >/dev/null || fail "the watcher survives a dead endpoint"
echo "ok US-89 field telemetry"
