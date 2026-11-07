#!/usr/bin/env bash
#
# Real-hardware stress gates (verify/stress/tests), release build, one test at a time, under
# nextest's timeout from .config/nextest.toml so a hang fails instead of sitting.
#
# Usage:
#   verify/scripts/stress.sh                     # every gate
#   verify/scripts/stress.sh concurrent          # one file
#   verify/scripts/stress.sh concurrent fill     # name filter
set -euo pipefail

cd "$(dirname "$0")/../.."

FILE="${1:-}"
FILTER="${2:-}"

echo ">>> stress  file='${FILE:-all}'  filter='${FILTER}'"

exec cargo nextest run -p xskiplist_stress --release --no-fail-fast --test-threads 1 ${FILE:+--test "${FILE}"} ${FILTER:+-E "test(${FILTER})"}
