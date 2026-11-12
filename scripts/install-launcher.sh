#!/usr/bin/env bash
# Install the `careerbrew` command (issue #304): a small launcher in ~/.local/bin that runs careerbrew from this
# checkout, wherever it lives. Commands and skills call `careerbrew ...` and read files from `$(careerbrew where)`,
# so no file hardcodes ~/apps/careerbrew. Safe to run again.
# Usage: scripts/install-launcher.sh [checkout]   (CAREERBREW_BIN_DIR overrides ~/.local/bin)
set -euo pipefail
root="$(cd "${1:-$(dirname "$0")/..}" && pwd -P)"
bin_dir="${CAREERBREW_BIN_DIR:-$HOME/.local/bin}"
launcher="$bin_dir/careerbrew"
marker="# careerbrew launcher, written by scripts/install-launcher.sh"

# A linked worktree has a .git file, not a folder: it must not take the launcher from the main checkout.
if [ -f "$root/.git" ] && [ "${CAREERBREW_LAUNCHER_FORCE:-0}" != 1 ]; then
  echo "skipped: $root is a linked worktree; the launcher stays on the main checkout"
  exit 0
fi
if [ -e "$launcher" ] && ! grep -qF "$marker" "$launcher"; then
  echo "kept: $launcher is not careerbrew's launcher; remove it and rerun to install"
  exit 0
fi

mkdir -p "$bin_dir"
tmp="$(mktemp "$bin_dir/.careerbrew.XXXXXX")"
cat >"$tmp" <<EOF
#!/bin/sh
$marker
root='$root'
if [ "\${1:-}" = where ]; then printf '%s\n' "\$root"; exit 0; fi
exec uv run --project "\$root" careerbrew "\$@"
EOF
chmod 755 "$tmp"
mv "$tmp" "$launcher"
echo "installed: $launcher -> $root"
case ":$PATH:" in *":$bin_dir:"*) ;; *) echo "note: add $bin_dir to PATH so agents and shells find \`careerbrew\`" ;; esac
