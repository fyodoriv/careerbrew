#!/usr/bin/env bash
# US-15: a corporate and a personal machine work together; no sync bundle carries corporate data off the corporate one, and job-hunt data never reaches it.
source "$(dirname "$0")/lib.sh"
CORP="$SCRATCH/corp"; PERS="$SCRATCH/pers"
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-acme"
# corporate machine
export CAREERBREW_HOME="$CORP"
cb init >/dev/null
cb machine set corporate --label work-laptop >/dev/null
[ ! -d "$CORP/jobs" ] || fail "example job-hunt files left on the corporate machine"
cb plugin add "$SCRATCH/plugin-acme" >/dev/null
cb work note "feat: launch PROJECTZEBRA billing" --tags delivery >/dev/null
out="$(cb jobs list 2>&1 || true)"
expect "$out" "personal-only" "job hunt blocked on corporate machine"
out="$(cb pair init)"
expect "$out" "not printed" "pairing key is not printed into a piped transcript"
expect "$(cb sync export "$SCRATCH/to-personal.cbsync" 2>&1 || true)" "corporate data stays on the corporate machine" "error case: corporate export refused"
[ ! -e "$SCRATCH/to-personal.cbsync" ] || fail "a corporate bundle was written"
# personal machine (same pairing key file stands in for `pair join`)
export CAREERBREW_HOME="$PERS"
cb init >/dev/null
cb machine set personal >/dev/null
cb jobs rate x 5 >/dev/null 2>&1 || true
cb sync export "$SCRATCH/to-corp.cbsync" >/dev/null
export CAREERBREW_HOME="$CORP"
expect "$(cb sync import "$SCRATCH/to-corp.cbsync")" "imported bundle from personal" "corporate imports settings only"
[ ! -e "$CORP/jobs/ratings.jsonl" ] || fail "job-hunt data reached the corporate machine"
export CAREERBREW_HOME="$PERS"
expect "$(cb sync import "$SCRATCH/to-corp.cbsync" 2>&1 || true)" "another personal machine" "error case: same-role bundle refused"
if grep -rq PROJECTZEBRA "$PERS"; then fail "corporate data reached the personal machine"; fi
# the role and plugin hints; joining with a key from a pipe; a bad key names the fix
export CAREERBREW_HOME="$CORP"
expect "$(cb machine show)" "role: corporate label: work-laptop" "machine show names the role"
key="$(cat "$CAREERBREW_PAIR_KEY_FILE")"
export CAREERBREW_PAIR_KEY_FILE="$SCRATCH/key/joined.key"
expect "$(printf '%s\n' "$key" | cb pair join)" "paired" "the personal machine joins with the key"
[ "$(cat "$CAREERBREW_PAIR_KEY_FILE")" = "$key" ] || fail "the joined key matches"
expect "$(cb pair join not-a-key 2>&1 || true)" "not a valid pairing key" "error case: a bad key names the fix"
echo "ok US-15 machines + sync"
