#!/usr/bin/env bash
# US-29: a second person at the same company (another team) installs careerbrew on a personal and a corporate machine.
source "$(dirname "$0")/lib.sh"
cp -R "$FIX/plugin-acme" "$SCRATCH/company-plugin"
cp -R "$FIX/plugin-acme-platform" "$SCRATCH/team-plugin"
# Person A (already a user): personal machine with the company plugin
export CAREERBREW_HOME="$SCRATCH/alice-personal"
cd "$ROOT" && SETUP_SKIP_DEPS=1 scripts/install.sh --role personal --plugin "$SCRATCH/company-plugin" >/dev/null
cb work note "Alice shipped the billing export" --tags delivery >/dev/null
# Person B: corporate machine with company + team plugin (team extends company)
export CAREERBREW_HOME="$SCRATCH/bob-corporate"
out="$(SETUP_SKIP_DEPS=1 scripts/install.sh --role corporate --label bob-work-laptop --plugin "$SCRATCH/company-plugin" --plugin "$SCRATCH/team-plugin")"
expect "$out" "acme-platform" "team plugin installed"
expect "$out" "role: corporate" "corporate role"
expect "$(cb ladder list)" "acme-platform-eng" "team ladder available"
data_cat settings.yaml | grep -q "ladder: acme-platform-eng" || fail "team plugin set the default ladder"
expect "$(cb plugin list)" '"export_to_personal": "aggregates"' "strictest export policy wins (team narrows company)"
expect "$(cb plugin remove acme 2>&1 || true)" "extended by ['acme-platform']" "error case: company plugin cannot be removed under its team plugin"
expect "$(cb jobs list 2>&1 || true)" "personal-only" "job hunt blocked on Bob's corporate machine"
if data_cat profile.yaml | grep -q "target_salary"; then fail "corporate profile has job-hunt fields"; fi
expect "$(cb machine ready 2>&1 || true)" "corporate readiness: " "readiness score printed"
ready="$(cb report latest)"  # the run page as text
expect "$ready" "done     profile.yaml exists and has no job-hunt fields" "corporate profile minimized"
expect "$ready" "blocker  profile.yaml has your real name" "readiness names what is still missing"
# Person B: personal machine
export CAREERBREW_HOME="$SCRATCH/bob-personal"
SETUP_SKIP_DEPS=1 scripts/install.sh --role personal >/dev/null
expect "$(cb work collect)" "0 new" "Bob's personal data root starts empty"
if grep -rq "Alice shipped" "$SCRATCH/bob-personal" "$SCRATCH/bob-corporate"; then fail "no data shared between people"; fi
# extends without the base installed fails cleanly
export CAREERBREW_HOME="$SCRATCH/carol"
SETUP_SKIP_DEPS=1 scripts/install.sh >/dev/null
expect "$(cb plugin add "$SCRATCH/team-plugin" 2>&1 || true)" "extends acme, which is not installed" "error case: team plugin needs the company plugin"
# Apple Silicon with an x86_64 venv: a clear error, never a silent exit 0 (issue #4)
printf '#!/bin/sh\necho x86_64\n' > "$SCRATCH/fake-py" && chmod +x "$SCRATCH/fake-py"
rc=0; out="$(HW_ARCH=arm64 VENV_PYTHON="$SCRATCH/fake-py" SETUP_SKIP_DEPS=1 scripts/install.sh 2>&1)" || rc=$?
[ "$rc" -ne 0 ] || fail "error case: wrong-arch venv must not exit 0"
expect "$out" "x86_64 on arm64 hardware" "error case: arch mismatch is named"
# A CLI that cannot start fails the install
printf '#!/bin/sh\n[ "$1" = --version ] && exit 1\nexit 0\n' > "$SCRATCH/broken-cb" && chmod +x "$SCRATCH/broken-cb"
rc=0; out="$(CAREERBREW_BIN="$SCRATCH/broken-cb" SETUP_SKIP_DEPS=1 scripts/install.sh 2>&1)" || rc=$?
[ "$rc" -ne 0 ] || fail "error case: broken CLI must not exit 0"
expect "$out" "careerbrew does not start" "error case: broken CLI is named"
echo "ok US-29 second person"
