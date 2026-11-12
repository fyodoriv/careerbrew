#!/usr/bin/env bash
set +x
# Keep the Git arguments out of the validator's argv; pass one bounded stream.
set -euo pipefail
if [ "$#" -ne 2 ]; then
  echo "pre-push context unavailable; retry through Git" >&2
  exit 1
fi
{ printf '%s\0%s\0' "$1" "$2"; cat; } | python3 scripts/verify-push-context.py
