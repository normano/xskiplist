#!/usr/bin/env bash
#
# Loom models (verify/loom/tests, one file per map).
#
# Usage:
#   verify/scripts/loom.sh                       # every model, 2 preemptions
#   verify/scripts/loom.sh concurrent            # one file
#   verify/scripts/loom.sh concurrent split 3    # name filter, deeper preemption bound
#   PREEMPT=3 verify/scripts/loom.sh             # env override
#
# `--cfg crossbeam_loom` makes crossbeam-epoch use loom's primitives as well, so the epoch pins
# and deferred frees are part of the model.
set -euo pipefail

cd "$(dirname "$0")/../.."

FILE="${1:-}"
FILTER="${2:-}"
PREEMPT="${3:-${PREEMPT:-2}}"
BRANCHES="${LOOM_MAX_BRANCHES:-1000000}"

echo ">>> loom  file='${FILE:-all}'  filter='${FILTER}'  max_preemptions=${PREEMPT}  max_branches=${BRANCHES}"

export LOOM_MAX_PREEMPTIONS="${PREEMPT}" LOOM_MAX_BRANCHES="${BRANCHES}"
export RUSTFLAGS="--cfg loom --cfg crossbeam_loom" CARGO_TARGET_DIR=target/loom

exec cargo test -p xskiplist_loom --release --no-fail-fast ${FILE:+--test "${FILE}"} -- ${FILTER:+"${FILTER}"}
