#!/usr/bin/env bash
# US-43 (issue #103): `data show` reads an encrypted file and changes nothing; `--keys` prints structure only.
source "$(dirname "$0")/lib.sh"
seed_jobs
cb data encrypt >/dev/null
printf 'connectors:\n  - {kind: jira, token: "keychain:careerbrew.jira", base_url: "https://projectzebra.example", port: 443}\n' | data_write work/connectors.yaml
printf 'projects:\n  projectzebra: {created_at: "2026-10-01T09:00:00Z", stale_after_days: 7}\n' | data_write work/projects.yaml
snapshot() { "$ROOT/.venv/bin/python" -c 'import hashlib, sys
from pathlib import Path
root = Path(sys.argv[1])
for p in sorted(root.rglob("*")):
    if p.is_file() and p.relative_to(root).parts[0] != "logs":
        print(p.relative_to(root), hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_mtime_ns)' "$CAREERBREW_HOME"; }
before="$(snapshot)"
expect "$before" "work/connectors.yaml" "the fixture file exists"
out="$(cb data status)"; refute "$out" "still plaintext" "every file is encrypted before show"

out="$(cb data show settings.yaml)"; expect "$out" "ladder: generic-swe" "an encrypted file prints decrypted"
out="$(cb data show private/settings.yaml)"; expect "$out" "ladder: generic-swe" "the documented private prefix resolves against the data root"
out="$(cb data show profile.yaml)"; expect "$out" "current_level: 3" "personal file prints"
refute "$out" "$CANARY_PERSON" "sensitive terms are redacted"; expect "$out" "[REDACTED]" "redaction is visible"

rc=0; out="$(cb data show work/connectors.yaml)" || rc=$?
[ "$rc" -eq 3 ] || fail "business text to a pipe must be refused with rc 3 (got $rc)"
expect "$out" "--keys" "the refusal points agents to --keys"; refute "$out" "projectzebra" "no business value leaks"
# macOS opens WORK/ as work/: refused there (rc 3), missing on a case-sensitive disk (rc 1); never rc 0.
rc=0; out="$(cb data show private/work/connectors.yaml)" || rc=$?
[ "$rc" -eq 3 ] || fail "the private prefix must preserve business refusal (got $rc)"; refute "$out" "projectzebra" "no leak through private prefix"
out="$(cb data show private/work/connectors.yaml --keys)"; expect "$out" "connectors[].port: int" "the private prefix preserves structure-only output"; refute "$out" "projectzebra" "no value in prefixed outline"
rc=0; out="$(cb data show WORK/connectors.yaml)" || rc=$?
[ "$rc" -ne 0 ] || fail "a business path in another case must not print"; refute "$out" "projectzebra" "no leak through another case"
out="$(cb data show work/projects.yaml --keys)"
expect "$out" "projects.*.created_at: str" "fields of records keyed by name"; refute "$out" "projectzebra" "a project name is never a key path"

out="$(cb data show work/connectors.yaml --keys)"
expect "$out" "work/connectors.yaml: yaml, encrypted on disk" "the header says the file is encrypted"
expect "$out" "connectors: list[1]" "list size"; expect "$out" "connectors[].base_url: str" "nested key and type"
expect "$out" "connectors[].port: int" "types, not values"
for v in projectzebra keychain jira 443; do refute "$out" "$v" "--keys never prints a value"; done

rc=0; out="$(cb data show ../outside.yaml)" || rc=$?
[ "$rc" -eq 1 ] || fail "a path outside the data root must be refused (got $rc)"; expect "$out" "no such data file" "names the problem"

[ "$(snapshot)" = "$before" ] || fail "data show changed a data file"
event_seen data.show
out="$(cb actions --json)"; expect "$out" "\"data.show\"" "agents find the action"
out="$(cb data set profile.yaml --name SYNTHETIC-USER --employer SYNTHETIC-EMPLOYER)"
expect "$out" "saved profile.yaml fields" "profile fields can be set without a terminal"
refute "$out" "SYNTHETIC-USER" "the result hides the name"
refute "$out" "SYNTHETIC-EMPLOYER" "the result hides the employer"
"$ROOT/.venv/bin/python" -c 'import sys; from pathlib import Path; from careerbrew import store, vault; p = Path(sys.argv[1]); d = store.read_yaml(p); assert d["name"] == "SYNTHETIC-USER" and d["employer"] == "SYNTHETIC-EMPLOYER" and vault.is_sealed(p.read_bytes())' "$CAREERBREW_HOME/profile.yaml"
event_seen data.set
echo "ok US-43 data show"
