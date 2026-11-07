#!/usr/bin/env bash
#
# ThreadSanitizer on the library's threaded tests and the Miri-sized scenarios, release build,
# nightly. `--cfg xskiplist_tsan` turns the consume-ordered tower loads into acquire loads: the
# dependency ordering they rely on is not something TSan can see, so without the cfg every
# descent is reported as a race. `--tests` leaves doctests out: they build without RUSTFLAGS and
# cannot link against the sanitized std.
#
# Usage:
#   verify/scripts/tsan.sh                 # library arena:: and concurrent:: tests plus verify/miri
#   verify/scripts/tsan.sh stress          # the stress gates under TSan (slow)
set -euo pipefail

cd "$(dirname "$0")/../.."

TARGET="$(rustc +nightly -vV | sed -n 's/^host: //p')"
export RUSTFLAGS="-Z sanitizer=thread --cfg xskiplist_tsan" CARGO_TARGET_DIR=target/tsan

echo ">>> tsan  target=${TARGET}  what='${1:-lib}'"

case "${1:-lib}" in
  lib)
    cargo +nightly test -Z build-std --target "$TARGET" --release -p xskiplist --lib -- arena:: concurrent::
    cargo +nightly test -Z build-std --target "$TARGET" --release -p xskiplist_miri --tests
    ;;
  stress)
    cargo +nightly test -Z build-std --target "$TARGET" --release -p xskiplist_stress --tests -- --test-threads=1
    ;;
  *)
    echo "unknown target '${1}'" >&2
    exit 2
    ;;
esac
