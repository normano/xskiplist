#!/usr/bin/env bash
#
# Miri over a seed range. Miri samples thread schedules, so one seed is one schedule; a sweep of
# seeds is the evidence. Tree Borrows because crossbeam-epoch trips Stacked Borrows inside its
# own thread-local list. Leaks are ignored because garbage still queued in crossbeam-epoch's
# global collector when the process ends is reported as leaked; every_value_dropped_once in the
# library tests covers reclamation.
#
# Usage:
#   verify/scripts/miri.sh                         # verify/miri tests, seeds 0..8
#   verify/scripts/miri.sh concurrent 0..32        # one file, more seeds
#   verify/scripts/miri.sh concurrent 0..8 arena   # name filter
#   verify/scripts/miri.sh lib                     # the library's arena:: and concurrent:: tests, one seed
set -euo pipefail

cd "$(dirname "$0")/../.."

FILE="${1:-}"
SEEDS="${2:-0..8}"
FILTER="${3:-}"
FLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks"

if [[ "$FILE" == "lib" ]]; then
  echo ">>> miri  library arena:: and concurrent:: tests"
  MIRIFLAGS="$FLAGS" exec cargo +nightly miri test -p xskiplist --lib -- arena:: concurrent::
fi

first="${SEEDS%..*}"
last="${SEEDS#*..}"
echo ">>> miri  file='${FILE:-all}'  seeds=${first}..${last}  filter='${FILTER}'"

status=0
for seed in $(seq "$first" $((last - 1))); do
  echo ">>> seed ${seed}"
  if ! MIRIFLAGS="$FLAGS -Zmiri-seed=${seed}" cargo +nightly miri test -p xskiplist_miri ${FILE:+--test "${FILE}"} -- ${FILTER:+"${FILTER}"}; then
    echo ">>> FAILED at seed ${seed}"
    status=1
  fi
done
exit "$status"
