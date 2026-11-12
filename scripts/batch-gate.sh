#!/usr/bin/env bash
# Batch gate: one full gate for up to 4 ready PRs, with no loss of stability.
#   scripts/batch-gate.sh [--dry-run] <pr>...
#
# 1. Refuse the batch (exit 2) when it holds more than BATCH_GATE_MAX (4) PRs,
#    or a PR that is not open, is a draft, does not target main, or touches a
#    gate file: scripts/check.sh, scripts/input-key.sh, scripts/e2e.sh,
#    tests/e2e/lib.sh, web/playwright*.config.ts.
#    Those PRs ship alone, with their own full gate.
#    A PR that conflicts with main + the PRs before it (a trial merge) leaves the
#    batch: the rest still gate and merge, and the summary names it (merge main
#    into it, then batch again). A conflict is never read as a test failure.
# 2. Solo: each PR head passes `scripts/check.sh fast` and its own e2e stories
#    (the tests/e2e/us-*.sh files it changes), alone.
# 3. Batch: merge origin/main and every PR head, in order, into a scratch
#    worktree, and run one full `scripts/check.sh` there (through heavy-run).
# 4. Pass: squash-merge the PRs in order (gh, --match-head-commit). After each
#    merge, main's tree must equal the scratch tree of that step. If it differs,
#    stop and rerun the full gate on main before any further merge (exit 3).
# 5. Fail: merge nothing (exit 1). Bisect by halves and name each PR that fails
#    alone; it goes back to its worker.
#
# --dry-run runs every check but merges nothing. Never pushes, never force-pushes,
# never uses --admin. The last line is one summary: "batch-gate: <verdict>: ...".
# Logs: .metrics/batch-gate/<stamp>/. Settings (env): BATCH_GATE_WORKTREE (the
# scratch worktree, reused so its setup and gate cache stay warm), BATCH_GATE_MAX,
# BATCH_GATE_REMOTE (origin), BATCH_GATE_E2E_PORT (the scratch gates' Playwright
# port, 8897, so a worker's own port never leaks in).
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2
# A git hook exports GIT_DIR and friends; git -C <worktree> must not write to this repo.
# shellcheck disable=SC2046
unset $(git rev-parse --local-env-vars)

start_s=$(date +%s)
dry=0
prs=()
for a in "$@"; do
  case "$a" in
    --dry-run) dry=1 ;;
    -h|--help) sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *[!0-9]*|'') echo "batch-gate: not a PR number: $a" >&2; exit 2 ;;
    *) prs+=("$a") ;;
  esac
done
max="${BATCH_GATE_MAX:-4}"
remote="${BATCH_GATE_REMOTE:-origin}"
conflict_note=""  # set by the trial merge; every summary after it names the PRs it left out
summary() { local m="$1$conflict_note"; echo "batch-gate: ${m//;;/;} ($(( $(date +%s) - start_s )) s; logs: $logdir)"; }
logdir=".metrics/batch-gate/$(date -u +%Y%m%dT%H%M%SZ)-$$"
mkdir -p "$logdir"

[ ${#prs[@]} -ge 1 ] || { echo "usage: scripts/batch-gate.sh [--dry-run] <pr>..." >&2; summary "refused: no PRs"; exit 2; }
[ ${#prs[@]} -le "$max" ] || { summary "refused: ${#prs[@]} PRs, a batch holds at most $max"; exit 2; }

# A gate file changes how every PR is judged, so a PR that touches one ships alone.
gate_file() {
  case "$1" in
    scripts/check.sh|scripts/input-key.sh|scripts/e2e.sh|tests/e2e/lib.sh|web/playwright*.config.ts) return 0 ;;
  esac
  return 1
}

if command -v heavy-run >/dev/null 2>&1; then heavy=(heavy-run); else
  echo "batch-gate: heavy-run is not on PATH, so the gates run without the Mac-wide lock" >&2; heavy=()
fi
export CAREERBREW_E2E_PORT="${BATCH_GATE_E2E_PORT:-8897}"
git_s=(git -c user.name=batch-gate -c user.email=batch-gate@localhost -c commit.gpgsign=false -c core.hooksPath=/dev/null)

# --- 1. Read each PR; refuse what may not batch -------------------------------
shas=() stories=()
refused="" slug=""
for i in "${!prs[@]}"; do
  n="${prs[$i]}"
  if ! gh pr view "$n" --json number,state,isDraft,baseRefName,headRefOid,url > "$logdir/pr-$n.json" 2> "$logdir/pr-$n.err"; then
    summary "refused: could not read PR #$n with gh ($(head -1 "$logdir/pr-$n.err"))"; exit 2
  fi
  # Lines: state, isDraft, base, head sha, HOST/OWNER/REPO (from the PR url; gh -R takes it on any host).
  info="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d["state"]); print(str(d["isDraft"]).lower()); print(d["baseRefName"]); print(d["headRefOid"]); print("/".join(d["url"].split("/")[2:5]))' "$logdir/pr-$n.json")" \
    || { summary "refused: could not parse PR #$n"; exit 2; }
  state="$(sed -n 1p <<< "$info")"; draft="$(sed -n 2p <<< "$info")"; base="$(sed -n 3p <<< "$info")"
  shas[i]="$(sed -n 4p <<< "$info")"; slug="$(sed -n 5p <<< "$info")"
  [ "$state" = OPEN ] || refused+=" #$n is $state;"
  [ "$draft" = false ] || refused+=" #$n is a draft;"
  [ "$base" = main ] || refused+=" #$n targets $base, not main;"
done
[ -z "$refused" ] || { summary "refused:${refused%;}"; exit 2; }

# --- Scratch worktree; fetch each PR head --------------------------------------------
main_root="$(git worktree list --porcelain | sed -n '1s/^worktree //p')"
wt="${BATCH_GATE_WORKTREE:-$main_root/../wt.noindex/cb-batch-scratch}"
git fetch -q "$remote" main || { summary "refused: git fetch $remote main failed"; exit 2; }
base_sha="$(git rev-parse "$remote/main")"
if [ ! -e "$wt/.git" ]; then
  git worktree add -q --detach "$wt" "$base_sha" > "$logdir/worktree.log" 2>&1 || { summary "refused: could not add the scratch worktree $wt"; exit 2; }
fi
for i in "${!prs[@]}"; do
  n="${prs[$i]}"
  git fetch -q "$remote" "+refs/pull/$n/head:refs/batch-gate/pr-$n" 2>> "$logdir/fetch.log" || { summary "refused: could not fetch PR #$n"; exit 2; }
  got="$(git rev-parse "refs/batch-gate/pr-$n")"
  [ "$got" = "${shas[$i]}" ] || { summary "refused: PR #$n head moved while reading it ($got, gh said ${shas[$i]})"; exit 2; }
  # The changed paths come from git, for the exact head that is gated (gh's file list is capped).
  # --no-renames: a renamed gate file must show its old path too. A failed diff refuses, never passes.
  paths="$(git diff --no-renames --name-only "$base_sha...${shas[$i]}")" || { summary "refused: could not list the files PR #$n changes"; exit 2; }
  s=""
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    gate_file "$path" && refused+=" #$n touches $path (ship it alone);"
    case "$path" in tests/e2e/us-*.sh) s+="$path " ;; esac
  done <<< "$paths"
  stories[i]="$s"
done
[ -z "$refused" ] || { summary "refused:${refused%;}"; exit 2; }

# --- Trial merge: a PR that conflicts with main + the PRs before it leaves the batch -----------
# A conflict is no test failure: the other PRs still gate and merge, and that PR goes back for a main merge.
conflicts=""
keep=()
git -C "$wt" checkout -q -f --detach "$base_sha" > "$logdir/trial-merge.log" 2>&1 || { summary "refused: could not check out main in $wt"; exit 2; }
for i in "${!prs[@]}"; do
  if "${git_s[@]}" -C "$wt" merge -q --no-ff --no-edit -m "batch-gate trial: PR #${prs[$i]}" "${shas[$i]}" >> "$logdir/trial-merge.log" 2>&1; then
    keep+=("$i")
  else
    if [ -z "$(git -C "$wt" ls-files -u)" ]; then  # no conflicted path: git refused for another reason (untracked files)
      git -C "$wt" merge --abort >> "$logdir/trial-merge.log" 2>&1
      summary "refused: could not trial-merge #${prs[$i]}, and not because of a conflict (see $logdir/trial-merge.log)"; exit 2
    fi
    git -C "$wt" merge --abort >> "$logdir/trial-merge.log" 2>&1
    conflicts+=" #${prs[$i]}"
    echo "batch-gate: #${prs[$i]} conflicts with main + the PRs before it: left out of this batch" >&2
  fi
done
conflict_note="${conflicts:+; not merged:$conflicts conflicts with main + the PRs before it (merge main into it, then batch again)}"
if [ ${#keep[@]} -eq 0 ]; then summary "failed: nothing merged"; exit 1; fi
kp=() ks=() kst=()
for i in "${keep[@]}"; do kp+=("${prs[$i]}"); ks+=("${shas[$i]}"); kst+=("${stories[$i]}"); done
prs=("${kp[@]}") shas=("${ks[@]}") stories=("${kst[@]}")

# The scratch worktree is this script's own: checkout -f drops any change an earlier gate left in tracked files.
# Setup reruns only when the dependency files change, so a reused worktree stays fast.
setup_if_needed() {
  local stamp
  stamp="$(cd "$wt" && cat uv.lock pyproject.toml web/package.json web/package-lock.json 2>/dev/null | git hash-object --stdin)"
  [ "$(cat "$wt/.metrics/batch-gate-setup" 2>/dev/null)" = "$stamp" ] && return 0
  (cd "$wt" && scripts/setup.sh) >> "$1" 2>&1 || return 1
  mkdir -p "$wt/.metrics" && echo "$stamp" > "$wt/.metrics/batch-gate-setup"
}

# --- 2. Solo checks ---------------------------------------------------------------
for i in "${!prs[@]}"; do
  n="${prs[$i]}" log="$logdir/solo-$n.log"
  echo "batch-gate: solo #$n: check.sh fast + stories: ${stories[$i]:-none}" >&2
  if ! { git -C "$wt" checkout -q -f --detach "${shas[$i]}" && setup_if_needed "$log" &&
         (cd "$wt" && ${heavy[@]+"${heavy[@]}"} bash -c 'scripts/check.sh fast && for s in "$@"; do [ ! -f "$s" ] || bash "$s" || exit 1; done' solo ${stories[$i]}); } >> "$log" 2>&1; then
    summary "failed: #$n fails its own fast gate or e2e stories alone, so nothing merged; it goes back to its worker ($log)"; exit 1
  fi
done

# --- 3. One full gate on main + the PRs ------------------------------------------------
trees=()      # trees[i]: the scratch tree after merging PRs 0..i (set by the last gate_set on all PRs)
gate_n=0
# gate_set <index>...: merge base + those PRs in the scratch worktree, then run the full gate.
gate_set() {
  local label="" i log
  for i in "$@"; do label+="${prs[$i]}-"; done
  gate_n=$((gate_n + 1)); log="$logdir/gate-$gate_n-${label%-}.log"
  trees=()
  git -C "$wt" checkout -q -f --detach "$base_sha" >> "$log" 2>&1 || return 1
  for i in "$@"; do
    if ! "${git_s[@]}" -C "$wt" merge -q --no-ff --no-edit -m "batch-gate: PR #${prs[$i]}" "${shas[$i]}" >> "$log" 2>&1; then
      git -C "$wt" merge --abort >> "$log" 2>&1
      echo "batch-gate: #${prs[$i]} conflicts with main + the PRs before it in this set" | tee -a "$log" >&2
      return 2
    fi
    trees[$i]="$(git -C "$wt" rev-parse 'HEAD^{tree}')"
  done
  echo "batch-gate: full gate on main + #${label%-} ($log)" >&2
  setup_if_needed "$log" || return 1
  (cd "$wt" && ${heavy[@]+"${heavy[@]}"} scripts/check.sh) >> "$log" 2>&1
}

all=("${!prs[@]}")
gate_set "${all[@]}"; rc=$?
# The trial merge already merged this set cleanly, so a 2 here means the worktree changed under the script.
[ "$rc" -ne 2 ] || { summary "stopped: the batch no longer merges in $wt, so nothing merged; run it again"; exit 1; }
if [ "$rc" -ne 0 ]; then
  # --- 5. Bisect by halves; merge nothing ----------------------------------------------
  alone="" together=""
  bisect() {  # bisect <index>...: the set failed; find who fails alone. gate_set 2: a half that cannot merge without the other.
    local k=$# half a b ra rb
    if [ "$k" -eq 1 ]; then alone+=" #${prs[$1]}"; return; fi
    half=$(( (k + 1) / 2 ))
    a=("${@:1:half}") b=("${@:half+1}")
    gate_set "${a[@]}"; ra=$?
    gate_set "${b[@]}"; rb=$?
    [ "$ra" -ne 1 ] || bisect "${a[@]}"
    [ "$rb" -ne 1 ] || bisect "${b[@]}"
    if [ "$ra" -ne 1 ] && [ "$rb" -ne 1 ]; then
      local l="" i; for i in "$@"; do l+="#${prs[$i]}+"; done; together+=" ${l%+}"
    fi
  }
  if [ ${#prs[@]} -eq 1 ]; then alone=" #${prs[0]}"; else bisect "${all[@]}"; fi
  msg="failed: nothing merged;"
  [ -z "$alone" ] || msg+=" fails alone:$alone (back to its worker);"
  [ -z "$together" ] || msg+=" fails only together:$together (ship them one at a time);"
  summary "${msg%;}"; exit 1
fi

if [ "$dry" = 1 ]; then
  summary "passed (dry run): would squash-merge $(printf '#%s ' "${prs[@]}")in order; nothing merged"; exit 0
fi

# --- 4. Merge in order; check main's tree after each merge ---------------------------------
# main_gate: run the full gate on main as it is now; sets verdict.
main_gate() {
  local log="$logdir/main-gate.log"
  if git fetch -q "$remote" main >> "$log" 2>&1 && git -C "$wt" checkout -q -f --detach "$remote/main" >> "$log" 2>&1 &&
     setup_if_needed "$log" && (cd "$wt" && ${heavy[@]+"${heavy[@]}"} scripts/check.sh) >> "$log" 2>&1; then
    verdict="main gate passed ($log)"
  else
    verdict="MAIN GATE FAILED: fix main first ($log)"
  fi
}
not_merged_after() { local j l=""; for j in "${!prs[@]}"; do [ "$j" -gt "$1" ] && l+=" #${prs[$j]}"; done; echo "$l"; }
git fetch -q "$remote" main || { summary "stopped: git fetch $remote main failed, so nothing merged; run the batch again"; exit 3; }
[ "$(git rev-parse "$remote/main")" = "$base_sha" ] || { summary "stopped: main moved since the gate, so nothing merged; run the batch again"; exit 3; }
merged=""
for i in "${!prs[@]}"; do
  n="${prs[$i]}"
  # -R: gh then touches no local branch (one may be checked out in a worker's worktree) and deletes only the remote one.
  gh pr merge "$n" -R "$slug" --squash --delete-branch --match-head-commit "${shas[$i]}" >> "$logdir/merge-$n.log" 2>&1
  # gh can exit nonzero after a merge, so read the state; a failed read is "unknown", never "not merged".
  state=""
  for try in 1 2 3; do
    state="$(gh pr view "$n" -R "$slug" --json state 2>> "$logdir/merge-$n.log" | python3 -c 'import json,sys; print(json.load(sys.stdin)["state"])' 2>/dev/null)" && [ -n "$state" ] && break
    [ "$try" = 3 ] || sleep "${BATCH_GATE_RETRY_SECONDS:-3}"
  done
  if [ "$state" != MERGED ]; then
    if [ -z "$merged" ] && [ -n "$state" ]; then
      summary "stopped: #$n did not merge (state $state), so nothing merged ($logdir/merge-$n.log)"; exit 1
    fi
    # main may now hold a tree no gate ran: gate it before anything else merges.
    main_gate
    left="${state:+ #$n}$(not_merged_after "$i")"
    summary "stopped: #$n did not merge (state ${state:-unknown: check it on GitHub}); $verdict;${merged:+ merged:$merged;}${left:+ not merged:$left (run a new batch)}"
    exit 3
  fi
  merged+=" #$n"
  if ! git fetch -q "$remote" main; then
    main_gate
    left="$(not_merged_after "$i")"
    summary "stopped: git fetch failed after #$n merged; $verdict; merged:$merged;${left:+ not merged:$left}"
    exit 3
  fi
  got="$(git rev-parse "$remote/main^{tree}")"
  if [ "$got" != "${trees[$i]}" ]; then
    echo "batch-gate: main's tree after #$n ($got) is not the gated tree (${trees[$i]}): full gate on main" >&2
    main_gate
    left="$(not_merged_after "$i")"
    summary "stopped: main's tree after #$n differs from the gated tree; $verdict; merged:$merged;${left:+ not merged:$left (run a new batch)}"
    exit 3
  fi
done
summary "passed: one full gate, then squash-merged$merged; main's tree equals the gated tree"
