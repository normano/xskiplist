#!/usr/bin/env bash
#
# Shuttle schedules (verify/shuttle/tests): randomised and PCT explorations at a scale loom
# cannot reach. Every atomic is sequentially consistent under shuttle, so this tier finds
# interleaving and liveness bugs, not ordering bugs.
#
# Usage:
#   verify/scripts/shuttle.sh                    # every schedule set
#   verify/scripts/shuttle.sh concurrent         # one file
#   verify/scripts/shuttle.sh concurrent pct     # name filter
set -euo pipefail

cd "$(dirname "$0")/../.."

FILE="${1:-}"
FILTER="${2:-}"

echo ">>> shuttle  file='${FILE:-all}'  filter='${FILTER}'"

export RUSTFLAGS="--cfg shuttle" CARGO_TARGET_DIR=target/shuttle

exec cargo test -p xskiplist_shuttle --release --no-fail-fast ${FILE:+--test "${FILE}"} -- ${FILTER:+"${FILTER}"}
