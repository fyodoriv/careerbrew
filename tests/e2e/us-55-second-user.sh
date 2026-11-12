#!/usr/bin/env bash
# US-55: a coworker sets themselves up on their own corporate machine with the shared company plugin, in one command.
source "$(dirname "$0")/lib.sh"
PY="$ROOT/.venv/bin/python"
# Synthetic names made at runtime: a fixed name in a tracked file would trip the repo-term check.
org="Zq$("$PY" -c 'import secrets, string; print("".join(secrets.choice(string.ascii_lowercase) for _ in range(8)))')"
owner_email="owner-$org@example.com" peer_email="peer-$org@example.com"
# The owner builds the shared plugin once (as in US-40).
export CAREERBREW_HOME="$SCRATCH/owner"
cb machine set corporate >/dev/null
plug="$SCRATCH/plugins/careerbrew-org"
cb plugin new "$org Corp" --dir "$plug" --connect jira,github --jira-url "https://jira.$org.example.com" --email "$owner_email" --github-user owner --no-prompt >/dev/null
"$PY" - "$plug" <<'PY'
import sys, pathlib, yaml
root = pathlib.Path(sys.argv[1])
lad = next((root / "ladders").glob("*.yaml"))
d = yaml.safe_load(lad.read_text())
names = {"mid": "L4", "senior": "L5", "staff": "L6"}
d["levels"] = [names.get(lv, lv) for lv in d["levels"]]
for c in d["competencies"]:
    c["expect"] = {names.get(k, k): v for k, v in c["expect"].items()}
lad.write_text(yaml.safe_dump(d, sort_keys=False))
m = yaml.safe_load((root / "careerbrew-plugin.yaml").read_text())
m["settings_defaults"]["target_level"] = "L5"
(root / "careerbrew-plugin.yaml").write_text(yaml.safe_dump(m, sort_keys=False))
PY
printf '%s-codename\n%s-internal.example.com\n' "$org" "$org" >> "$plug/sensitive-terms.txt"
git -C "$plug" add -A && git -C "$plug" -c user.name=t -c user.email=t@example.com commit -qm "feat: company plugin"
if grep -rqF "$owner_email" "$plug"; then fail "the owner's email is in the shared plugin"; fi
# A coworker: fresh home, their own repos, one command.
export CAREERBREW_HOME="$SCRATCH/peer"
mkdir -p "$SCRATCH/peer-apps/svc-a" "$SCRATCH/peer-apps/svc-b"
for r in svc-a svc-b; do git -C "$SCRATCH/peer-apps/$r" init -q; done
out="$(cb machine new-user --plugin "$plug" --repos "$SCRATCH/peer-apps" --email "$peer_email" --name "$CANARY_PERSON" --github-user peer 2>&1)" || { printf '%s\n' "$out" >&2; cb report latest >&2; fail "new-user reaches readiness"; }
expect "$out" "2 git repo(s) as evidence sources" "the coworker's repos found"
total="$(printf '%s' "$out" | sed -n 's/.*corporate readiness: \([0-9]*\)\/\([0-9]*\).*/\1 \2/p')"
[ -n "$total" ] && [ "${total% *}" = "${total#* }" ] || { printf '%s\n' "$out" >&2; fail "readiness is N/N (got: $total)"; }
page="$(cb report latest)"  # the run page as text
expect "$page" "Type your jira token in your terminal" "token steps for the coworker"
expect "$page" "-> careerbrew secret set careerbrew.p-" "each with its exact command"
expect "$page" "gh issue create" "how to report bugs from a pull-only machine"
refute "$page" "$peer_email" "the coworker's email stays off the agent summary"
mine="$("$PY" -c 'from careerbrew import paths, store; print(store.read_text(paths.data_root()/"work"/"connectors.yaml")); print(store.read_text(paths.data_root()/"work"/"sources.yaml"))')"
expect "$mine" "$peer_email" "the coworker's own email"
case "$mine" in *"$owner_email"*|*"owner"*) fail "an owner value reached the coworker's settings";; esac
# Error case: no plugin installed.
export CAREERBREW_HOME="$SCRATCH/nobody"
expect "$(cb machine new-user --email x@example.com 2>&1 || true)" "install the company plugin first" "error case: no plugin"
echo "ok US-55 second user"
