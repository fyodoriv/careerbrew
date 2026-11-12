#!/usr/bin/env bash
# US-12: install a corporate plugin, use its ladder and policy, remove it cleanly.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-acme"
expect "$(cb plugin add "$SCRATCH/plugin-acme")" "added plugin acme 1.0.0 (plugin:acme), exec_allowed=False" "add"
out="$(cb plugin list)"
expect "$out" "acme" "listed"
expect "$out" '"allowed_routes": ["local"]' "policy merged"
expect "$(cb ladder list)" "acme-eng" "plugin ladder available"
expect "$(cb plugin verify)" "ok  acme" "verify clean"
echo tamper >> "$CAREERBREW_HOME/plugins/acme/rules.md"
out="$(cb plugin verify || true)"
expect "$out" "TAMPERED  acme" "error case: tamper detected"
cb plugin add "$SCRATCH/plugin-acme" --allow-exec >/dev/null
expect "$(cb work collect)" "1 new" "exec-allowed command source"
expect "$(cb plugin remove acme --purge-data)" "purged 1 record(s)" "remove purges plugin data"
refute "$(cb ladder list)" "acme-eng" "ladder gone after remove"
[ ! -d "$CAREERBREW_HOME/plugins/acme" ] || fail "plugin files removed"
echo "ok US-12 plugin"
