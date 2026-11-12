#!/usr/bin/env bash
# US-55: the owner of an older company plugin moves its per-person sources and connectors into private files, as they are (issue #102).
source "$(dirname "$0")/lib.sh"
PY="$ROOT/.venv/bin/python"
# Synthetic names made at runtime: a fixed name in a tracked file would trip the repo-term check.
org="Zq$("$PY" -c 'import secrets, string; print("".join(secrets.choice(string.ascii_lowercase) for _ in range(8)))')"
owner_email="owner-$org@example.com"
export CAREERBREW_HOME="$SCRATCH/owner"
cb machine set corporate >/dev/null
plug="$SCRATCH/plugins/careerbrew-org"
cb plugin new "$org Corp" --dir "$plug" --connect jira --jira-url "https://jira.$org.example.com" --email "$owner_email" --no-prompt >/dev/null
# The old shape (before issue #48): hand-picked repos authored by the owner and a tuned connector in the shared file.
cat > "$plug/evidence.yaml" <<YAML
sources:
  - {kind: git, path: '$SCRATCH/apps/svc-a', authors: ['$owner_email']}
  - {kind: git, path: '$SCRATCH/hand-picked/svc-b', authors: ['$owner_email']}
  - {kind: gh, host: 'github.$org.example.com'}
connectors:
  - {kind: jira, base_url: 'https://gateway.$org.example.com/jira', jql: 'project = OPS AND assignee = currentUser()', email: '$owner_email', token: 'keychain:careerbrew.p-old.jira'}
YAML
git -C "$plug" add -A && git -C "$plug" -c user.name=t -c user.email=t@example.com commit -qm "feat: company plugin"
cb plugin refresh >/dev/null
cb machine ready >/dev/null 2>&1 || true  # it prints its run page path; `report latest` is that page as text
expect "$(cb report latest)" "careerbrew machine split-plugin-evidence" "readiness names the migration as the fix"

# Dry run: the counts it would reach, nothing written.
before="$(cat "$plug/evidence.yaml")"
out="$(cb machine split-plugin-evidence --dry-run 2>&1)"
expect "$out" "would move 2 source(s), 1 connector(s)" "dry run counts"
expect "$out" "evidence sources work collect reads: 5 -> 5" "dry run keeps the total"
[ "$(cat "$plug/evidence.yaml")" = "$before" ] || fail "the dry run changed the plugin"

# The move: per-person entries go to private files as they are; the shared one stays.
out="$(cb machine split-plugin-evidence 2>&1)" || { printf '%s\n' "$out" >&2; fail "split-plugin-evidence exits 0"; }
expect "$out" "evidence.yaml: 4 evidence entries -> 1 shared (moved 2 source(s), 1 connector(s))" "counts per plugin file"
expect "$out" "work/sources.yaml: 0 -> 2 source(s); work/connectors.yaml: 1 -> 2 connector(s)" "your files before and after"
expect "$out" "evidence sources work collect reads: 5 -> 5" "no evidence source lost"
expect "$out" "ok    the company plugin evidence file holds no per-person values" "the readiness row passes"
refute "$out" "$owner_email" "counts only, never the moved values"
mine="$(data_cat work/sources.yaml; data_cat work/connectors.yaml)"
expect "$mine" "$SCRATCH/hand-picked/svc-b" "the hand-picked repo kept"
expect "$mine" "https://gateway.$org.example.com/jira" "the tuned base_url kept"
expect "$mine" "project = OPS AND assignee = currentUser()" "the custom jql kept"
if grep -qF "$owner_email" "$plug/evidence.yaml"; then fail "the owner's email is still in the plugin"; fi
expect "$(cat "$plug/evidence.yaml")" "github.$org.example.com" "the shared source stays"
refute "$(cb plugin verify 2>&1)" "TAMPERED" "reinstalled from its folder: the lock hash matches"
cb machine ready >/dev/null 2>&1 || true
expect "$(cb report latest)" "done     the company plugin evidence file holds no per-person values" "machine ready agrees"
event_seen machine.split_evidence
expect "$(cb machine split-plugin-evidence 2>&1)" "nothing to move" "a rerun moves nothing"
[ "$(data_cat work/sources.yaml | grep -c "kind: git")" = 2 ] || fail "a rerun added a duplicate"

# Error case: a plugin installed from a git URL has no folder here to edit (HEAD still has the old shape).
export CAREERBREW_HOME="$SCRATCH/from-url"
cb machine set corporate >/dev/null
cb plugin add "file://$plug" >/dev/null
out="$(cb machine split-plugin-evidence 2>&1)" && rc=0 || rc=$?
[ "$rc" = 1 ] || { printf '%s\n' "$out" >&2; fail "a plugin with no folder here exits 1 (got $rc)"; }
expect "$out" "careerbrew plugin add <its folder>" "error case: names the fix"
[ ! -e "$CAREERBREW_HOME/work/sources.yaml" ] || fail "nothing moves when the plugin cannot be edited"
echo "ok US-55 split plugin evidence"
