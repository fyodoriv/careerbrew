#!/usr/bin/env bash
# US-89: anonymous usage counts go to a telemetry endpoint only with a key, never with content; a fake local endpoint only.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
# Error case: with no key, nothing is sent and status says why.
expect "$(cb telemetry status)" "telemetry: off (no key" "error case: no key, nothing is sent"
expect "$(cb telemetry send)" "nothing sent" "error case: send with no key sends nothing"
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
export CAREERBREW_TELEMETRY_URL="http://127.0.0.1:$(cat "$PORT_FILE")" CAREERBREW_POSTHOG_KEY="phc_synthetic_e2e_key"
# Real runs with business text: it must never leave.
cb work task "Prepare $CANARY_EMPLOYER offer for $CANARY_PERSON" --hours 1 >/dev/null
expect "$(cb telemetry status)" "telemetry: on (on)" "a key turns it on"
dry="$(cb telemetry send --dry-run)"
expect "$dry" '"api_key": "<posthog_key>"' "the dry run hides the key"
expect "$dry" '"command": "work.task"' "the dry run shows the command counts"
refute "$dry" "phc_synthetic_e2e_key" "privacy: the dry run never prints the key"
[ ! -s "$BODIES" ] || fail "a dry run sends nothing"
expect "$(cb telemetry send)" "sent " "send posts the batch"
sent="$(cat "$BODIES")"
expect "$sent" '"distinct_id"' "events carry the random install id"
for leak in "$CANARY_EMPLOYER" "$CANARY_PERSON" "$CAREERBREW_HOME" "$(hostname)"; do refute "$sent" "$leak" "privacy: no content, path, or host leaves"; done
expect "$(cb telemetry send)" "sent " "a second send works"
[ "$(grep -c '"work.task"' "$BODIES")" = 1 ] || fail "nothing is sent twice"
# Off switches.
n="$(wc -l <"$BODIES")"
expect "$(DO_NOT_TRACK=1 cb telemetry send)" "DO_NOT_TRACK is set" "DO_NOT_TRACK stops it"
expect "$(cb telemetry off)" "telemetry off" "telemetry off"
expect "$(cb telemetry send)" "nothing sent" "off sends nothing"
[ "$(wc -l <"$BODIES")" = "$n" ] || fail "no request while off"
# A dead endpoint never fails another command.
cb telemetry on >/dev/null
CAREERBREW_TELEMETRY_URL="http://127.0.0.1:9" cb day watch once >/dev/null || fail "the watcher survives a dead endpoint"
echo "ok US-89 field telemetry"
