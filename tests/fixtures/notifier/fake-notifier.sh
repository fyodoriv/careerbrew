#!/usr/bin/env bash
# A stand-in for alerter and terminal-notifier in tests (issue #74). Link it as `alerter` or
# `terminal-notifier` in a folder at the front of PATH; it acts as the tool it is named after.
#   FAKE_NOTIFIER_LOG    file: each call appends "--- <tool>" and then one argument per line
#   FAKE_NOTIFIER_CLICK  alerter: the label it prints, as if clicked (Open, Accept, Dismiss, Snooze,
#                        @CONTENTCLICKED); terminal-notifier: "execute" runs the -execute command
# It never shows anything and never reaches the network.
set -euo pipefail
tool="$(basename "$0")"
{ printf -- '--- %s\n' "$tool"; printf '%s\n' "$@"; } >> "${FAKE_NOTIFIER_LOG:?set FAKE_NOTIFIER_LOG}"
click="${FAKE_NOTIFIER_CLICK:-}"
[ -n "$click" ] || exit 0
if [ "$tool" = alerter ]; then
  printf '%s\n' "$click"
elif [ "$tool" = terminal-notifier ] && [ "$click" = execute ]; then
  while [ $# -gt 0 ]; do
    if [ "$1" = -execute ]; then sh -c "$2"; exit $?; fi
    shift
  done
fi
