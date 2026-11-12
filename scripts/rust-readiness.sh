#!/usr/bin/env bash
# Shared local-only bootstrap check. Sourcing owns RUST_READY_BIN in this shell.

rust_readiness_fail() {
  echo "FAIL  $1 -> fix: $2"
  return 1
}

rust_resolve_executable() {
  local path="$1" target count=0
  while [ -L "$path" ]; do
    count=$((count + 1))
    [ "$count" -le 40 ] || return 1
    target="$(readlink "$path")" || return 1
    case "$target" in /*) path="$target" ;; *) path="$(dirname "$path")/$target" ;; esac
  done
  [ -f "$path" ] && [ -x "$path" ] || return 1
  printf '%s/%s\n' "$(cd "$(dirname "$path")" && pwd -P)" "$(basename "$path")"
}

rust_readiness() {
  local root pin selected manager compiler bin metadata component version override
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)" || return 1
  # Accept the repository's explicit release/minimal/component shape only.
  # Unknown TOML settings need review before a bootstrap can act on them.
  pin="$(awk '
    { sub(/#.*/, ""); gsub(/^[ \t]+|[ \t]+$/, "") }
    /^$/ { next }
    /^\[toolchain\]$/ { section++; next }
    /^channel = "[0-9]+\.[0-9]+\.[0-9]+"$/ { if (section!=1) bad=1; channel++; value=$3; gsub(/"/, "", value); next }
    /^profile = "minimal"$/ { if (section!=1) bad=1; profile++; next }
    /^components = \["clippy", "rustfmt"\]$/ { if (section!=1) bad=1; components++; next }
    { bad=1 }
    END { if (!bad && section==1 && channel==1 && profile==1 && components==1) print value; else exit 1 }
  ' "$root/rust-toolchain.toml" 2>/dev/null)" || {
    rust_readiness_fail "Rust pin is malformed or unsupported" "restore rust-toolchain.toml or review its new settings"; return 1;
  }
  for override in RUSTC RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER RUSTUP_TOOLCHAIN; do
    if [ -n "${!override:-}" ]; then
      rust_readiness_fail "Rust override $override selects unverified tools" "run setup/doctor with $override unset"; return 1
    fi
  done
  selected="$(command -v rustc)" || {
    rust_readiness_fail "Rust compiler is missing" "install Rust $pin with clippy and rustfmt, then rerun"; return 1;
  }
  compiler="$(rust_resolve_executable "$selected")" || {
    rust_readiness_fail "Rust compiler path is unavailable" "repair the installed Rust $pin toolchain"; return 1;
  }
  manager="$(command -v rustup || true)"
  if { [ -n "$manager" ] && [ "$compiler" -ef "$manager" ]; } || [ "$(basename "$compiler")" = rustup ]; then
    # Rustup hardlinks dispatch by basename. Query the manager as rustup.
    if [ -z "$manager" ] || ! [ "$compiler" -ef "$manager" ]; then manager="$compiler"; fi
    selected="$(RUSTUP_AUTO_INSTALL=0 "$manager" which --toolchain "$pin" rustc 2>/dev/null)" || {
      rust_readiness_fail "Rust $pin toolchain is not installed" "install Rust $pin with clippy and rustfmt, then rerun"; return 1;
    }
    case "$selected" in /*) ;; *) rust_readiness_fail "Rust compiler discovery failed" "repair the local Rustup installation"; return 1 ;; esac
    compiler="$(rust_resolve_executable "$selected")" || {
      rust_readiness_fail "Rust compiler discovery returned an unavailable file" "repair the local Rustup installation"; return 1;
    }
    if [ "$compiler" -ef "$manager" ] || [ "$(basename "$compiler")" = rustup ]; then
      rust_readiness_fail "Rust compiler discovery returned a proxy" "repair the local Rustup installation"; return 1
    fi
  fi
  bin="$(dirname "$compiler")"
  metadata="$(RUSTUP_AUTO_INSTALL=0 "$compiler" -Vv 2>/dev/null)" || metadata=""
  if [ "$(printf '%s\n' "$metadata" | sed -n 's/^release: //p')" != "$pin" ]; then
    rust_readiness_fail "Rust compiler release differs from $pin" "select the installed Rust $pin bin directory on PATH"; return 1
  fi
  for component in cargo rustfmt clippy-driver; do
    selected="$(rust_resolve_executable "$bin/$component")" || {
      rust_readiness_fail "Rust component $component is missing" "install $component in the Rust $pin toolchain"; return 1;
    }
    if [ "$(dirname "$selected")" != "$bin" ] || { [ -n "$manager" ] && [ "$selected" -ef "$manager" ]; }; then
      rust_readiness_fail "Rust component $component is outside the compiler installation" "select one complete Rust $pin installation"; return 1
    fi
    version="$(RUSTUP_AUTO_INSTALL=0 "$selected" --version 2>/dev/null)" || version=""
    case "$component:$version" in
      "cargo:cargo $pin "*|rustfmt:rustfmt\ [0-9]*|clippy-driver:clippy\ [0-9]*) ;;
      *) rust_readiness_fail "Rust component $component has an invalid version (Cargo must match $pin)" "repair the Rust $pin installation"; return 1 ;;
    esac
  done
  RUST_READY_BIN="$bin"
  echo "ok    Rust $pin compiler, Cargo, rustfmt and Clippy installed"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  rust_readiness
fi
