#!/usr/bin/env bash
# US-81: sign in to Outlook once with a device code; the token refreshes itself, and an ended sign-in asks to reconnect.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CAREERBREW_NOW="2026-10-19T09:00+00:00"
# A stand-in for macOS `security`, so the story never touches the real Keychain: one file per service.
kc="$SCRATCH/keychain" && mkdir -p "$kc" "$SCRATCH/bin"
cat > "$SCRATCH/bin/security" <<STUB
#!/usr/bin/env bash
if [ "\$1" = "-i" ]; then read -r -a w; svc="\${w[3]}"; printf '%s' "\${w[7]}" > "$kc/\$svc"; exit 0; fi
svc="\$3"; [ -f "$kc/\$svc" ] && cat "$kc/\$svc" && echo || exit 44
STUB
chmod +x "$SCRATCH/bin/security" && export PATH="$SCRATCH/bin:$PATH"
cb init >/dev/null
# Error case: with no app registration, connect names the setting and the manual token.
out="$(cb connect outlook 2>&1 || true)"
expect "$out" "careerbrew config set outlook_client_id" "no client ID: the fix names the setting"
expect "$out" "expires after about an hour" "the manual token path says it expires"
cb config set outlook_client_id synthetic-client >/dev/null
out="$(CAREERBREW_TEST_OWNER= cb connect outlook 2>&1 || true)"  # an agent path: no owner terminal
expect "$out" "in your own terminal" "the sign-in code is not written to a pipe"
out="$(cb connect outlook)"
expect "$out" "enter the code SYNTH123" "the device code is shown"
expect "$out" "connector outlook saved with secret reference keychain" "the refresh token goes to the Keychain"
[ "$(cat "$kc/careerbrew.outlook.refresh")" = "synthetic-refresh-token-2" ] || fail "the refresh token was not stored"
if data_has "synthetic-refresh-token" || data_has "synthetic-access-token"; then fail "a token reached the data root"; fi
expect "$(cb day plan)" "1 meeting(s)" "the next read refreshes the token and reads the calendar"
# Error case: Microsoft ends the sign-in; the read shows one plain step, not empty data.
"$ROOT/.venv/bin/python" -c 'from careerbrew import paths, store; p = paths.data_root() / "work" / "connectors.yaml"; d = store.read_yaml(p); d["connectors"][0]["tenant"] = "synthetic-expired-tenant"; store.write_yaml(p, d)'
cb day plan >/dev/null 2>&1 || true
expect "$(cb report latest)" "Reconnect Outlook: run \`careerbrew connect outlook\` in your terminal" "an ended sign-in asks to reconnect"
echo "ok US-81 outlook sign-in"
