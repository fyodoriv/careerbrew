#!/usr/bin/env bash
# Run the regression evals. CHECK_EVALS=1 scripts/check.sh calls this file.
# Deterministic graders first (evals/run.py): skill structure, act/refrain coverage,
# and CLI cases that run against synthetic data. LLM-judged cases run only when
# EVALS_LLM=1 and a local model is up (config/models.yaml judge route).
set -euo pipefail
cd "$(dirname "$0")/.."
exec uv run --quiet python evals/run.py "$@"
