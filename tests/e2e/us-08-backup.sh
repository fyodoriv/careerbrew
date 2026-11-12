#!/usr/bin/env bash
# US-08: encrypted, verified backups I can restore.
source "$(dirname "$0")/lib.sh"
seed_jobs
out="$(cb backup create)"
expect "$out" "backup written and verified" "backup create verifies"
f="$(ls "$CAREERBREW_HOME/../backups/"*.cbk)"
if grep -qa "$CANARY_PERSON" "$f"; then fail "backup is not encrypted"; fi
expect "$(cb backup verify)" "decrypts, hash matches" "verify latest"
cb backup restore "$f" "$SCRATCH/restored" >/dev/null
grep -q "$CANARY_PERSON" "$SCRATCH/restored/profile.yaml" || fail "restore brings data back"
printf 'x' >> "$f"
out="$(cb backup verify "$f" || true)"
expect "$out" "failed verification" "error case: tampered backup is caught"
# opt-in iCloud Drive target (a temp folder stands in for iCloud Drive/careerbrew)
export CAREERBREW_ICLOUD_DIR="$SCRATCH/icloud/careerbrew"
cb backup create >/dev/null
[ -e "$CAREERBREW_ICLOUD_DIR" ] && fail "iCloud backup is off until the user opts in"
cb config set backup_icloud true >/dev/null
cb backup create >/dev/null
g="$(ls "$CAREERBREW_ICLOUD_DIR/"*.cbk)"
if grep -qa "$CANARY_PERSON" "$g"; then fail "iCloud backup is not encrypted"; fi
cb backup restore "$g" "$SCRATCH/from-icloud" >/dev/null
grep -q "$CANARY_PERSON" "$SCRATCH/from-icloud/profile.yaml" || fail "restore from the iCloud copy brings data back"
echo "ok US-08 backup"
