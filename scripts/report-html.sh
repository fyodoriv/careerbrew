#!/usr/bin/env bash
# Turn a checkup log into one self-contained HTML page: the verdict, the
# blockers, what to do next, and every section in full.
# Usage: scripts/report-html.sh "<title>" <log-file> <out.html>
#        scripts/report-html.sh "<title>" <section-dir> <out.html> <exit code>
# Log shape: "== <section>" or "## <section>" starts a section; a line
# starting with "->" or "Next:" is a next step; a line with FAIL, ✗, "not ok", or "failed" marks its section as a
# blocker; the last "checkup: ..." line is the verdict.
# A section folder (scripts/checkup.sh writes one: NN-<id>.title, .cmd, .log, .rc) gives a richer page,
# kept as a run page under private/out/reports/ (`careerbrew report latest --json`) and copied to <out.html>.
# The page stays local (write it under an ignored folder such as .metrics/).
# One renderer: this calls careerbrew's Python renderer (src/careerbrew/report.py), which every run page uses.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -x "$root/.venv/bin/python" ]; then
  exec "$root/.venv/bin/python" -m careerbrew.report html "$@"
fi
exec uv run --quiet --project "$root" python -m careerbrew.report html "$@"
