#!/usr/bin/env bash
# Batched migration measurement (docs/ci.md, "Batched migration measurement"). The manual workflow
# .github/workflows/migration-measure.yml runs baseline, verify, measure and decide for each step on a hosted
# runner and uploads the four content-free JSON records as one artifact per step.
# Usage:
#   scripts/migration-records.sh steps                    every step with a registered Rust candidate (xtask/src/measure.rs)
#   scripts/migration-records.sh matrix "<steps>" <macos> the workflow's job matrix as JSON; empty steps means every step
#   scripts/migration-records.sh run <step> <pairs> <dir> on the runner: build the candidate's package, write the four records in <dir>
#   scripts/migration-records.sh fetch <run-id>           on your machine: download a run's records into .metrics/migration/
#   scripts/migration-records.sh fetch --dir <dir>        the same, from artifacts already downloaded into <dir>
set -euo pipefail
# MIGRATION_RECORDS_ROOT: tests point the script at a scratch folder.
ROOT="${MIGRATION_RECORDS_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
REGISTRY="xtask/src/measure.rs"
WORKFLOW=".github/workflows/migration-measure.yml"
RECORDS="baseline verify measure decide"
STEP_ID='^rust-[0-9]{2}[a-z]?$'
# An artifact name: the Linux job uploads migration-<step>, the macOS job migration-<step>-macos-15.
ARTIFACT='^migration-(rust-[0-9]{2}[a-z]?)(-macos-15)?$'

die() { echo "migration-records: $*" >&2; exit 2; }

steps() {
  # The registry lists each candidate as `step: "rust-NN",` in registry order.
  sed -nE 's/^[[:space:]]*step: "(rust-[0-9]{2}[a-z]?)",$/\1/p' "$ROOT/$REGISTRY"
}

registered() {
  steps | grep -qxF -- "$1"
}

matrix() {
  local wanted="${1:-}" macos="${2:-false}" step first=1
  local -a chosen=() oses=(ubuntu-24.04)
  case "$macos" in true) oses+=(macos-15) ;; false) ;; *) die "matrix takes true or false for macOS (got: $macos)" ;; esac
  if [ -z "${wanted//[[:space:]]/}" ]; then
    while IFS= read -r step; do chosen+=("$step"); done < <(steps)
  else
    for step in $wanted; do
      [[ "$step" =~ $STEP_ID ]] || die "not a step id: $step"
      registered "$step" || die "step $step has no registered Rust candidate in $REGISTRY"
      chosen+=("$step")
    done
  fi
  [ "${#chosen[@]}" -gt 0 ] || die "no step to measure"
  printf '{"include":['
  for step in "${chosen[@]}"; do
    for os in "${oses[@]}"; do
      [ "$first" -eq 1 ] || printf ','
      first=0
      local name="migration-$step"
      [ "$os" = ubuntu-24.04 ] || name="$name-$os"
      printf '{"step":"%s","os":"%s","artifact":"%s"}' "$step" "$os" "$name"
    done
  done
  printf ']}\n'
}

# The candidate's path relative to the repo root, read from the registry entry of the step.
candidate() {
  awk -v want="$1" '
    $0 ~ "step: \"" want "\"," { found = 1; next }
    found && /program: "/ { sub(/.*program: "/, ""); sub(/",.*/, ""); print; exit }
  ' "$ROOT/$REGISTRY"
}

run() {
  local step="${1:-}" pairs="${2:-}" out="${3:-}" program bin profile status=0
  [[ "$step" =~ $STEP_ID ]] || die "run needs a step id (got: ${step:-nothing})"
  registered "$step" || die "step $step has no registered Rust candidate in $REGISTRY"
  [[ "$pairs" =~ ^[1-9][0-9]*$ ]] || die "run needs a positive number of pairs (got: ${pairs:-nothing})"
  [ -n "$out" ] || die "run needs an output folder"
  [[ "$out" = /* ]] || die "the output folder must be an absolute path: $out"
  [ ! -e "$out" ] || die "the output folder must be new: $out"
  program="$(candidate "$step")"
  [[ "$program" =~ ^target/(release|debug)/[A-Za-z0-9_-]+$ ]] || die "cannot read the candidate of $step from $REGISTRY"
  bin="${program##*/}"
  profile="${program#target/}"
  profile="${profile%%/*}"
  mkdir -p "$out"
  cd "$ROOT"
  local python="$ROOT/.venv/bin/python"
  local -a release=()
  [ "$profile" = release ] && release=(--release)
  cargo build --locked -p careerbrew-xtask
  # Build every bin of the candidate's package: some verifies need a helper next to the candidate
  # (rust-22 runs exercise-fixture-runner).
  local package
  package="$(cargo metadata --locked --no-deps --format-version 1 \
    | jq -r --arg bin "$bin" '.packages[] | select(any(.targets[]; .name == $bin and (.kind | index("bin")))) | .name')"
  [ -n "$package" ] || die "no workspace package builds the bin $bin"
  cargo build --locked ${release[@]+"${release[@]}"} -p "$package" --bins
  local -a few=()
  [ "$pairs" -ge 30 ] || few=(--test-allow-few-pairs)
  # verify exits 1 on a differing case and decide exits 1 on keep-python: both still write their record.
  cargo xtask migration baseline --step "$step" --python "$python" --out "$out/baseline.json" || status=$?
  cargo xtask migration verify --step "$step" --python "$python" --out "$out/verify.json" || status=$?
  cargo xtask migration measure --step "$step" --python "$python" --pairs "$pairs" ${few[@]+"${few[@]}"} --out "$out/measure.json" || status=$?
  if [ -f "$out/baseline.json" ] && [ -f "$out/verify.json" ] && [ -f "$out/measure.json" ]; then
    cargo xtask migration decide --step "$step" --baseline "$out/baseline.json" --verify "$out/verify.json" \
      --measure "$out/measure.json" --out "$out/decide.json" || [ $? -eq 1 ] || status=2
  fi
  summary "$step" "$out"
  # A missing record or a failed verify fails the job; keep-python alone does not.
  for record in $RECORDS; do [ -f "$out/$record.json" ] || return 1; done
  [ "$(jq -r '.passed' "$out/verify.json")" = true ] || return 1
  [ "$status" -ne 2 ] || return 1
}

# One line per step: the step, the verify result and the decide verdict.
summary() {
  local step="$1" dir="$2" verify=missing decision=missing reasons=""
  [ -f "$dir/verify.json" ] && verify="$(jq -r 'if .passed == true then "pass" elif .passed == false then "fail" else "unknown" end' "$dir/verify.json")"
  if [ -f "$dir/decide.json" ]; then
    decision="$(jq -r '.decision // "unknown"' "$dir/decide.json")"
    reasons="$(jq -r '(.reasons // []) | join(",")' "$dir/decide.json")"
  fi
  printf '%s verify=%s decide=%s%s\n' "$step" "$verify" "$decision" "${reasons:+ reasons=$reasons}"
}

# Check one downloaded artifact folder and copy its records into .metrics/migration/<step>[/macos-15].
take() {
  local name="$1" from="$2" step os dest record file
  [[ "$name" =~ $ARTIFACT ]] || die "refused artifact name: $name"
  step="${BASH_REMATCH[1]}"
  os="${BASH_REMATCH[2]#-}"
  # Only the four records may travel: a file with another name is refused before anything is written.
  for file in "$from"/* "$from"/.[!.]*; do
    [ -e "$file" ] || continue
    case "${file##*/}" in baseline.json | verify.json | measure.json | decide.json) ;; *) die "$name holds an unexpected file: ${file##*/}" ;; esac
    [ -f "$file" ] && [ ! -L "$file" ] || die "$name holds a file that is not a regular file: ${file##*/}"
  done
  for record in $RECORDS; do
    file="$from/$record.json"
    [ -f "$file" ] || continue
    jq -e --arg step "$step" --arg schema "careerbrew-migration-$record-v1" \
      '.schema == $schema and .step == $step' "$file" > /dev/null 2>&1 \
      || die "$name: $record.json is not a $record record of $step"
  done
  dest="$ROOT/.metrics/migration/$step${os:+/$os}"
  mkdir -p "$dest"
  for record in $RECORDS; do
    rm -f "$dest/$record.json"
    [ -f "$from/$record.json" ] && cp "$from/$record.json" "$dest/$record.json"
  done
  summary "$step${os:+ ($os)}" "$dest"
}

fetch() {
  local dir name n=0
  if [ "${1:-}" = --dir ]; then
    dir="${2:?fetch --dir needs a folder}"
    [ -d "$dir" ] || die "no such folder: $dir"
  else
    [[ "${1:-}" =~ ^[0-9]+$ ]] || die "fetch needs a run id (gh run list --workflow migration-measure.yml) or --dir <folder>"
    # Not a local: the EXIT trap reads it after the function returns.
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/migration-records.XXXXXX")"
    trap 'rm -rf "$tmp"' EXIT
    local origin
    origin="$(gh api "repos/{owner}/{repo}/actions/runs/$1" --jq '[.path, .event, (.head_repository.full_name == .repository.full_name | tostring)] | join(" ")')" || die "cannot read run $1"
    [ "$origin" = "$WORKFLOW workflow_dispatch true" ] || die "run $1 is not a measurement run of this repository (gh run list --workflow migration-measure.yml)"
    while IFS= read -r name; do
      [[ "$name" =~ $ARTIFACT ]] || continue
      gh run download "$1" --name "$name" --dir "$tmp/$name" || die "cannot download $name of run $1"
    done < <(gh api "repos/{owner}/{repo}/actions/runs/$1/artifacts" --paginate --jq '.artifacts[] | select(.expired | not) | .name')
    dir="$tmp"
  fi
  for name in "$dir"/*; do
    [ -d "$name" ] || continue
    take "${name##*/}" "$name"
    n=$((n + 1))
  done
  [ "$n" -gt 0 ] || die "no migration record artifact found"
  echo "migration-records: wrote $n artifact(s) into .metrics/migration/"
}

case "${1:-}" in
  steps) steps ;;
  matrix) shift; matrix "$@" ;;
  run) shift; run "$@" ;;
  fetch) shift; fetch "$@" ;;
  *) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
