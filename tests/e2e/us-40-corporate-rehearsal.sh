#!/usr/bin/env bash
# US-40: rehearse docs/corporate-machine.md end to end on a synthetic org, and reach full readiness.
source "$(dirname "$0")/lib.sh"
PY="$ROOT/.venv/bin/python"
# Random synthetic names: a fixed name would sit in a tracked file and fail the repo-term check.
org="Zq$("$PY" -c 'import secrets, string; print("".join(secrets.choice(string.ascii_lowercase) for _ in range(8)))')"
# 1. Get the code: option B, a git bundle, then block pushes.
git -C "$ROOT" bundle create "$SCRATCH/careerbrew.bundle" main >/dev/null 2>&1
git -c advice.detachedHead=false clone -q "$SCRATCH/careerbrew.bundle" "$SCRATCH/clone" || fail "bundle clone works"
git -C "$SCRATCH/clone" remote set-url --push origin no-push-from-corporate
expect "$(git -C "$SCRATCH/clone" remote get-url --push origin)" "no-push-from-corporate" "pushes blocked"
# 2. Install with the corporate role (run from this checkout: the clone has no venv).
out="$(cd "$ROOT" && SETUP_SKIP_DEPS=1 scripts/install.sh --role corporate --label work-laptop)"
expect "$out" "role: corporate" "corporate role set"
expect "$(cb machine ready 2>&1 || true)" "machine-ready: blocked: " "error case: a fresh install is not ready yet"
expect "$(cb report latest)" "-> fix: " "and its page says why, with each fix"
# Data files are encrypted at rest; the runbook uses `careerbrew data edit` (it needs a terminal), so the
# rehearsal edits through the same store careerbrew reads.
set_data() { "$PY" - "$@" <<'PY'
import sys
from careerbrew import paths, store
name, *pairs = sys.argv[1:]
p = paths.data_root() / name
d = store.read_yaml(p, {}) or {}
d.update(dict(pair.split("=", 1) for pair in pairs))
store.write_yaml(p, d)
PY
}
set_data profile.yaml "name=$CANARY_PERSON" "employer=$CANARY_EMPLOYER"
# 3. Build the company plugin and fill it.
plug="$SCRATCH/plugins/careerbrew-org"
cb plugin new "$org Corp" --dir "$plug" --connect jira,github --jira-url https://jira.$org.example.com --github-user me --no-prompt >/dev/null
expect "$(cb report latest)" "careerbrew secret set careerbrew.p-" "token commands listed for the owner to run"
ladder="$(ls "$plug"/ladders/*.yaml | head -1)"
"$PY" - "$ladder" <<'PY'
import sys, yaml
p = sys.argv[1]
d = yaml.safe_load(open(p))
# Stand-in for the promo-research step: real level names and expectations.
names = {"mid": "L4", "senior": "L5", "staff": "L6"}
d["levels"] = [names.get(lv, lv) for lv in d["levels"]]
for c in d["competencies"]:
    c["expect"] = {names.get(k, k): v for k, v in c["expect"].items()}
    c["how_to_close"] = "Synthetic rehearsal guidance for " + c["name"]
yaml.safe_dump(d, open(p, "w"), sort_keys=False)
PY
printf '%s-codename\n%s-internal.example.com\n' "$org" "$org" >> "$plug/sensitive-terms.txt"
level="$("$PY" -c "import sys,yaml; d=yaml.safe_load(open(sys.argv[1])); print(d['levels'][1])" "$ladder")"
set_data settings.yaml "target_level=$level"
git -C "$plug" add -A && git -C "$plug" -c user.name=t -c user.email=t@example.com commit -qm "feat: company plugin"
cb plugin refresh >/dev/null
# 5. Prove it.
ready="$(cb machine ready 2>&1)" || { printf '%s\n' "$ready" >&2; cb report latest >&2; fail "machine ready exits 0"; }
expect "$ready" "machine-ready: ok: " "no failing readiness check"
total="$(printf '%s' "$ready" | sed -n 's/.*corporate readiness: \([0-9]*\)\/\([0-9]*\).*/\1 \2/p')"
[ -n "$total" ] && [ "${total% *}" = "${total#* }" ] || fail "readiness is N/N (got: $total)"
refute "$ready" "$org" "company name redacted from output"
expect "$(cb jobs list 2>&1 || true)" "personal-only" "job hunt blocked on the corporate machine"
# 5. The runbook's push block passes the checkup (issue #13).
CAREERBREW_REPO_DIR="$SCRATCH/clone" cb checkup --offline >/dev/null 2>&1 || { cb report latest >&2; fail "checkup --offline passes on a runbook clone"; }
expect "$(cb report latest)" "done     pull-only clone" "runbook push block accepted"
echo "ok US-40 corporate rehearsal (readiness ${total% *}/${total#* })"
