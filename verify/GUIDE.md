# verify: validation tiers for the concurrent maps

Unit and regression tests live with the code (`src/**/tests.rs`). Everything that needs an external tool, a special build or a long run lives here, one crate per tier, all members of the workspace. Each tier catches a class of defect the others cannot, so a change to `ArenaSkipMap` or `ConcurrentSkipMap` runs every tier.

| Tier | Crate | Runner | Catches |
|---|---|---|---|
| 1 | `xskiplist` test modules | `cargo test --lib` | behaviour against a `BTreeMap` oracle, 8-thread stress at small scale |
| 2 | `verify/stress` | `verify/scripts/stress.sh [file] [filter]` | anything the real machine does at scale: lost entries, hangs, reclamation leaks |
| 3 | `verify/shuttle` | `verify/scripts/shuttle.sh [file] [filter]` | interleaving and liveness bugs at realistic scale, randomised and PCT schedules, every atomic sequentially consistent |
| 4 | `verify/loom` | `verify/scripts/loom.sh [file] [filter] [preemptions]` | exhaustive small models of the split, unlink and same-key protocols |
| 5 | `verify/miri` | `verify/scripts/miri.sh [file] [seeds] [filter]` | undefined behaviour, data races, use-after-free, aliasing, one schedule per seed |
| 6 | ThreadSanitizer | `verify/scripts/tsan.sh [lib or stress]` | data races under real scheduling on every core |
| 7 | `MODEL.md` | reading | the orderings no checker here models |

## What each tier cannot see

- **Stress** finds what the OS scheduler happens to produce. A pass is evidence, not proof.
- **Shuttle** treats every atomic as sequentially consistent, so a missing acquire or release looks correct to it.
- **Loom** models acquire and release exactly but has no consume ordering; the maps run their tower loads as acquire loads under it (`sync::MODELED`). Models must stay tiny: node capacity is 2, towers are capped at 3 levels, arena chunks are 1 KiB, two spawned threads, a handful of keys. The node lock is loom's mutex under it, so a thread waiting for a node blocks instead of spinning through the model. crossbeam-epoch runs on loom's primitives through `--cfg crossbeam_loom`.
- **Miri** samples schedules, so run many seeds. It has no dependency ordering either and gets acquire loads. crossbeam-epoch needs Tree Borrows. Garbage still queued in its collector at exit shows up as a leak, which is what the flags in the script are for.
- **TSan** sees the C++ model only: the consume-ordered loads are a race to it, so `tsan.sh` builds with `--cfg xskiplist_tsan`, which also turns them into acquire loads. What it then checks is everything else: the seqlock, the node locks and the epoch frees under real contention.
- **MODEL.md** is only as good as the reading; each entry names what it rests on.

## Instrumented primitives

The maps take their atomics, mutex, thread-locals and spin hint from `src/sync/`. Normal builds re-export std and parking_lot. Under `--cfg loom` they are loom's types, under `--cfg shuttle` shuttle's. `src/sync/` also holds the model parameters: node capacity (`CAP`, 32 normally, 2 under loom, 4 under shuttle), tower cap (`TOWER_CAP`, 24 normally, 3 under loom, 6 under shuttle) and arena chunk size. Under a checker tower heights come from a fixed seed and every thread uses one arena stripe, so executions are deterministic. Anything that blocks or spins must come from the facade. Otherwise a model run stalls on a real block. Payload reads through raw pointers stay on std; races there are Miri's and TSan's job.

## Running

One expensive tier at a time. Loom, shuttle, Miri, TSan, stress and the benchmarks each saturate the machine; two together slow both and make timing-dependent failures appear and disappear. Each tier uses its own target directory under `target/`, so switching tiers does not rebuild the others.

The library's own threaded tests shrink 400x under Miri (`test_util::scaled_mt`); the `verify/miri` scenarios are sized for a seed sweep from the start.
