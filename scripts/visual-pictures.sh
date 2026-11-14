#!/usr/bin/env bash
# The Linux pictures of the visual tests (US-75) are drawn on the hosted Linux runner, the one place that compares
# them (docs/web.md, "Linux pictures"). A runner uploads no artifact (docs/ci.md), so a picture travels in the run's
# log: its path, its SHA-256, and its bytes in base64.
# Usage:
#   scripts/visual-pictures.sh draw [changed|all|none]  on the runner (.github/workflows/visual-pictures.yml):
#                                                       redraw, compare, then print each new or changed picture
#   scripts/visual-pictures.sh fetch <run-id>           on your machine: write the pictures of that run here
#   scripts/visual-pictures.sh fetch --log <file>       the same, from a saved `gh run view <run-id> --log`
#   scripts/visual-pictures.sh print <picture>...       print pictures the way `draw` does (paths from the repo root)
set -euo pipefail
# VISUAL_PICTURES_ROOT: tests point the script at a scratch folder.
ROOT="${VISUAL_PICTURES_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
SHOTS="web/visual/__screenshots__"
INVENTORY="docs/proof/public-readiness-binaries.tsv"
# The only paths `fetch` writes. A log is text anyone's run can print, so a path is checked, never trusted.
PICTURE='^web/visual/__screenshots__/[A-Za-z0-9_-][A-Za-z0-9._-]*\.spec\.ts/[A-Za-z0-9_-][A-Za-z0-9._-]*-(desktop|phone)-linux\.png$'

die() { echo "visual-pictures: $*" >&2; exit 1; }
sha() { shasum -a 256 "$1" | cut -c1-64; }

print_pictures() {
  local path
  for path in "$@"; do
    [[ "$path" =~ $PICTURE ]] || die "not a Linux picture of a visual test: $path"
    printf 'visual-picture-begin %s %s\n' "$path" "$(sha "$ROOT/$path")"
    base64 < "$ROOT/$path" | tr -d '\n' | fold -w 1000
    printf '\nvisual-picture-end %s\n' "$path"
  done
}

draw() {
  local mode="${1:-changed}" status=0 path
  local -a changed=()
  case "$mode" in changed | all | none) ;; *) die "draw takes changed, all or none (got: $mode)" ;; esac
  cd "$ROOT/web"
  [ "$(node -p process.platform)" = linux ] || die "draw runs on the hosted Linux runner; on a Mac use: npm run visual:update"
  # What draws the pixels. When every picture fails at once in the gate, compare these lines with docs/ci.md.
  echo "visual-environment image ${ImageOS:-unknown} ${ImageVersion:-unknown}"
  echo "visual-environment playwright $(npx playwright --version)"
  echo "visual-environment sans-serif $(fc-match -f '%{family} %{fontversion} %{file}' 'Liberation Sans')"
  echo "visual-environment monospace $(fc-match -f '%{family} %{fontversion} %{file}' monospace)"
  if [ "$mode" != none ]; then
    npx playwright test -c playwright.visual.config.ts --reporter=line --update-snapshots="$mode" \
      || echo "visual-pictures: the drawing run failed; the comparison below decides"
  fi
  # A picture counts only when a second, fresh run draws the same pixels.
  npx playwright test -c playwright.visual.config.ts --reporter=line || status=1
  cd "$ROOT"
  while IFS= read -r path; do
    [ -f "$path" ] && changed+=("$path")
  done < <(git status --porcelain --untracked-files=all -- "$SHOTS" | cut -c4- | grep -- '-linux\.png$' || true)
  [ "${#changed[@]}" -eq 0 ] || print_pictures "${changed[@]}"
  if [ "$status" -eq 0 ]; then
    echo "visual-pictures: ok, every screen matches its picture (mode $mode, ${#changed[@]} new or changed picture(s) printed)"
  else
    echo "visual-pictures: FAILED, a screen differs from its picture (mode $mode, ${#changed[@]} new or changed picture(s) printed)"
  fi
  return "$status"
}

fetch() {
  local log n=0 path want file
  # Not a local: the EXIT trap reads it after the function returns.
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/visual-pictures.XXXXXX")"
  trap 'rm -rf "$tmp"' EXIT
  if [ "${1:-}" = --log ]; then
    log="${2:?fetch --log needs a file}"
  else
    [[ "${1:-}" =~ ^[0-9]+$ ]] || die "fetch needs a run id (gh run list --workflow visual-pictures.yml) or --log <file>"
    log="$tmp/log"
    gh run view "$1" --log > "$log" || die "cannot read the log of run $1"
  fi
  # A log line is `<job> TAB <step> TAB <time> <text>`: the text of a picture line is its last field.
  awk -v out="$tmp" '
    { sub(/\r$/, "") }
    /visual-picture-begin / { n++; sub(/.*visual-picture-begin /, ""); print $1 "\t" $2 "\t" n > (out "/list"); file = out "/" n ".b64"; printf "" > file; next }
    /visual-picture-end /   { if (file != "") close(file); file = ""; next }
    file != ""              { print $NF > file }
  ' "$log"
  [ -s "$tmp/list" ] || die "the log holds no picture (a run prints only new or changed pictures)"
  # First check every picture, then write: a bad log changes no file.
  while IFS=$'\t' read -r path want file; do
    [[ "$path" =~ $PICTURE ]] || die "refused path in the log: $path"
    [[ "$want" =~ ^[0-9a-f]{64}$ ]] || die "no SHA-256 for $path"
    tr -d '\n' < "$tmp/$file.b64" | base64 --decode > "$tmp/$file.png" 2>/dev/null || die "bad base64 for $path"
    [ "$(sha "$tmp/$file.png")" = "$want" ] || die "$path does not have the SHA-256 the run printed (a cut log?)"
    [ "$(head -c 8 "$tmp/$file.png" | od -An -tx1 | tr -d ' \n')" = 89504e470d0a1a0a ] || die "$path is not a PNG"
  done < "$tmp/list"
  while IFS=$'\t' read -r path want file; do
    mkdir -p "$ROOT/$(dirname "$path")"
    cp "$tmp/$file.png" "$ROOT/$path"
    n=$((n + 1))
    # The public-readiness inventory holds one row per non-text file (surface S7).
    if [ -f "$ROOT/$INVENTORY" ]; then
      { head -1 "$ROOT/$INVENTORY"
        { tail -n +2 "$ROOT/$INVENTORY" | awk -F'\t' -v p="$path" '$1 != p'
          printf '%s\tgenerated\t%s\t%s\n' "$path" "$want" "$(dirname "$path" | sed 's|/__screenshots__||')"
        } | LC_ALL=C sort
      } > "$tmp/inventory"
      cp "$tmp/inventory" "$ROOT/$INVENTORY"
    fi
  done < "$tmp/list"
  echo "visual-pictures: wrote $n picture(s) and their rows in $INVENTORY; open each picture before you commit it"
}

case "${1:-}" in
  draw) shift; draw "$@" ;;
  fetch) shift; fetch "$@" ;;
  print) shift; print_pictures "$@" ;;
  *) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
