#!/usr/bin/env bash
# Repo-wide checkup: the whole health picture in one command, open work first.
# Fast on a quiet day (the gate is cached by input hash), full when anything changed.
# Run it before a work session and in the weekly checkup (RECURRING.md).
#
# With the run_pages setting on (the default), each section's text goes into one HTML page
# (Done, Blockers, Next) made by scripts/report-html.sh: kept under private/out/reports/, with a
# copy at .metrics/checkup.html (git-ignored). The output is the page path plus one summary line;
# agents read `careerbrew report latest --json`. With it off
# (`uv run careerbrew config set run_pages false`), the sections print as text, as before.
set -uo pipefail
cd "$(dirname "$0")/.."

py=(.venv/bin/python)
[ -x .venv/bin/python ] || py=(uv run --quiet python)
pages=1
"${py[@]}" -m careerbrew.report enabled >/dev/null 2>&1 || pages=0

rc=0
exec 3>&2
logs="$(mktemp -d "${TMPDIR:-/tmp}/careerbrew-checkup.XXXXXX")"
trap 'rm -rf "$logs"' EXIT
n=0
section() {
  # section <id> <title> <command shown on the page> <function>: run one section, keep its text and exit code.
  local id="$1" title="$2" cmd="$3" fn="$4" f code
  n=$((n + 1))
  f="$logs/$(printf '%02d' "$n")-$id"
  printf '%s\n' "$title" > "$f.title"
  printf '%s\n' "$cmd" > "$f.cmd"
  if [ "$pages" = 1 ]; then
    printf '.. %s\n' "$title" >&2
    "$fn" > "$f.log" 2>&1
    code=$?
  else
    printf '\n== %s\n' "$title"
    "$fn" 2>&1 | tee "$f.log"
    code=${PIPESTATUS[0]}
  fi
  echo "$code" > "$f.rc"
  return "$code"
}

# When this checkup last ran on this machine and at which commit (issue #232): ${XDG_STATE_HOME:-~/.local/state}/checkups/careerbrew.json
last_run() {
  local text
  text="$("${py[@]}" -m careerbrew.checkup_runs last careerbrew . 2>&1)"
  printf '%s\n' "$text"
  [ "$pages" = 1 ] && printf '%s\n' "$text" >&3  # the real stderr: a section's own stderr goes to its log
  return 0
}

record_run() { "${py[@]}" -m careerbrew.checkup_runs record careerbrew "$([ "$rc" = 0 ] && echo green || echo blocked)" . >/dev/null 2>&1 || true; }

open_prs() { scripts/open-prs.sh; }

open_work_skill() {
  local open_work="${WRAP_UP_SKILL:-$HOME/.claude/skills/wrap-up}/scripts/open-work.sh"
  if [ -f "$open_work" ]; then
    bash "$open_work" . && return 0
    echo "-> finish these first: conflicts, red checks, review comments, then merge (wrap-up skill, \"Continue open work\")"
    return 1
  fi
  git status --short
  echo "(wrap-up skill not installed; showed git status; open PRs are listed above)"
}

health() { scripts/doctor.sh; }

gate() { scripts/check.sh; }

broken_stories() {
  if [ -f .metrics/events.jsonl ]; then
    local broken
    broken="$(grep '"ok":false' .metrics/events.jsonl | grep -oE '"story":"[^"]+"' | sort | uniq -c | sort -rn || true)"
    if [ -n "$broken" ]; then echo "$broken"; echo "-> each one is a P0 task"; else echo "none"; fi
  else
    echo "no events yet"
  fi
}

speed() { scripts/metrics.sh; }

open_work() { scripts/open-work.sh; }

p0_tasks() {
  [ -f TASKS.md ] || return 0
  awk '/^## P0/{f=1; next} /^## /{f=0} f && /^- \[ \]/' TASKS.md | sed 's/^- \[ \] /- /'
}

audit() {
  local skill="${NEW_PROJECT_SKILL:-$HOME/.claude/skills/new-project}"
  if [ -x "$skill/scripts/audit-due.sh" ]; then
    if "$skill/scripts/audit-due.sh" .; then
      echo "-> run it: $skill/references/requirements-map.md, section 4"
    fi
  else
    echo "new-project skill not installed; skipped"
  fi
}

section last-run "Last checkup on this machine" "python -m careerbrew.checkup_runs last careerbrew" last_run || true
section open-prs "Open pull requests (finish these first)" "scripts/open-prs.sh" open_prs || rc=1
section open-work-skill "Open work (unmerged branches, uncommitted files)" "wrap-up skill: scripts/open-work.sh" open_work_skill || rc=1
section health "Health" "scripts/doctor.sh" health || rc=1
section gate "Gate (cached by input hash; NO_CACHE=1 forces a full run)" "scripts/check.sh" gate || rc=1
section broken-stories "Broken stories (failed events by story)" "grep '\"ok\":false' .metrics/events.jsonl" broken_stories || true
section speed "Speed" "scripts/metrics.sh" speed || true
section open-work "Open work (PRs, branches, unpushed or uncommitted changes)" "scripts/open-work.sh" open_work || true
section p0 "Open P0 tasks" "TASKS.md, section P0" p0_tasks || true
section audit "Monthly requirements audit" "new-project skill: scripts/audit-due.sh" audit || true

record_run
if [ "$pages" = 1 ]; then
  bash scripts/report-html.sh "careerbrew checkup" "$logs" .metrics/checkup.html "$rc" && exit "$rc"
  echo "checkup: could not write the run page; the section text follows" >&2
  for f in "$logs"/*.log; do printf '\n== %s\n' "$(cat "${f%.log}.title")"; cat "$f"; done
fi
printf '\ncheckup: %s\n' "$([ "$rc" = 0 ] && echo healthy || echo "needs work (see above)")"
exit "$rc"
