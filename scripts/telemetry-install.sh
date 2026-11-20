# Setup's install numbers for field telemetry (US-89; docs/observability.md, "Field telemetry").
# Source it, call telemetry_install_begin first and telemetry_install_ok after a passing doctor.
# It only saves numbers locally (`careerbrew telemetry record-install`); they leave the machine
# only after the person says yes. It records only with this checkout's own `.venv/bin/careerbrew`
# (never another checkout's launcher on PATH), never fails setup, and never hides setup's own exit code.

# This checkout's own careerbrew command (both scripts run from the repo root).
_telemetry_cb() {
  if [ -x .venv/bin/careerbrew ]; then .venv/bin/careerbrew "$@"; else return 1; fi
}

telemetry_install_begin() {
  TELEMETRY_INSTALL_STARTED_MS="$(($(date +%s 2>/dev/null || echo 0) * 1000))"
  trap '_telemetry_install_rc=$?; if [ "$_telemetry_install_rc" -ne 0 ]; then _telemetry_cb telemetry record-install --started "$TELEMETRY_INSTALL_STARTED_MS" --ok false >/dev/null 2>&1 || true; fi; exit "$_telemetry_install_rc"' EXIT
}

telemetry_install_ok() {
  _telemetry_cb telemetry record-install --started "$TELEMETRY_INSTALL_STARTED_MS" --ok true >/dev/null 2>&1 || true
}
