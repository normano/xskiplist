# Concurrent map benchmarks

How `ConcurrentSkipMap` and `ArenaSkipMap` compare with `crossbeam-skiplist` and with two lock-wrapped maps, how the numbers were taken and how to take them again.

## Contents

* [Results](#results)
  * [Point reads](#point-reads)
  * [Mixed reads and writes](#mixed-reads-and-writes)
  * [Range scans under writes](#range-scans-under-writes)
  * [Filling an empty map](#filling-an-empty-map)
* [Methodology](#methodology)
* [Reproduction](#reproduction)

## Results

Every cell is nanoseconds per operation per thread: the time a thread spends on one of its own operations while the other threads run theirs. Lower is better. A flat row across thread counts means the structure scales. The five numbers in a cell are 1 / 2 / 4 / 8 / 14 threads on a 14-core machine (10 performance cores, 4 efficiency cores).

The competitors are `crossbeam-skiplist` 0.1 (`SkipMap`), `parking_lot::RwLock<BTreeMap>` and `parking_lot::Mutex<xskiplist::SkipMap>`. The map holds 100k keys. All runs were taken on 2026-11-07, one pass of 10 s per cell, in high-power mode, with the machine otherwise idle apart from the run itself.

### Point reads

Half the lookups hit, half miss.

| map | 1 / 2 / 4 / 8 / 14 threads |
|---|---|
| ConcurrentSkipMap | 116.7 / 121.2 / 122.9 / 128.8 / 160.4 |
| ArenaSkipMap | 126.1 / 128.8 / 128.6 / 130.7 / 158.2 |
| crossbeam-skiplist | 121.6 / 128.7 / 139.0 / 143.6 / 184.1 |
| RwLock<BTreeMap> | 67.2 / 86.8 / 207.9 / 811.2 / 3584.5 |
| Mutex<SkipMap> | 98.1 / 266.5 / 726.5 / 1546.2 / 2552.0 |

The lock-wrapped maps are fastest alone and collapse as soon as two threads contend. The three lock-free readers stay within 1.5x of their single-thread cost at 14 threads.

### Mixed reads and writes

Each operation picks a random key; a write is an insert or a remove with equal probability. `ArenaSkipMap` cannot remove, so it has no row here.

| workload | map | 1 / 2 / 4 / 8 / 14 threads |
|---|---|---|
| 90% reads | ConcurrentSkipMap | 135.1 / 145.0 / 150.3 / 146.9 / 194.5 |
| | crossbeam-skiplist | 179.5 / 195.3 / 210.9 / 237.4 / 335.2 |
| | RwLock<BTreeMap> | 67.4 / 112.8 / 294.9 / 1258.1 / 3370.5 |
| | Mutex<SkipMap> | 102.6 / 304.5 / 765.3 / 1607.3 / 2486.0 |
| 50% reads | ConcurrentSkipMap | 146.8 / 169.2 / 196.3 / 225.2 / 283.8 |
| | crossbeam-skiplist | 235.7 / 288.3 / 348.7 / 463.7 / 760.4 |
| | RwLock<BTreeMap> | 71.4 / 161.5 / 458.4 / 1407.3 / 3493.2 |
| | Mutex<SkipMap> | 106.3 / 328.4 / 814.2 / 1754.2 / 2807.3 |
| writes only | ConcurrentSkipMap | 157.5 / 173.2 / 205.0 / 268.8 / 401.3 |
| | crossbeam-skiplist | 298.2 / 372.4 / 527.2 / 748.7 / 1301.9 |
| | RwLock<BTreeMap> | 73.1 / 211.5 / 506.7 / 1102.8 / 1891.1 |
| | Mutex<SkipMap> | 101.4 / 300.9 / 784.1 / 1682.6 / 2641.1 |

Writers in `ConcurrentSkipMap` lock one node of 32 entries, so two writers collide only when they land in the same node. crossbeam-skiplist links one node per entry with a compare-and-swap per level. Its write cost grows with every thread added.

### Range scans under writes

Each scan reads 100 consecutive entries from a random start. From two threads up, one thread inserts and removes while the others scan.

| map | 1 / 2 / 4 / 8 / 14 threads |
|---|---|
| ConcurrentSkipMap | 501.4 / 258.5 / 435.1 / 579.6 / 891.7 |
| ArenaSkipMap | 253.4 / 103.8 / 151.7 / 195.9 / 259.2 |
| crossbeam-skiplist | 777.4 / 532.6 / 1004.9 / 2148.2 / 4087.7 |
| RwLock<BTreeMap> | 300.0 / 206.5 / 563.5 / 2337.4 / 3426.6 |
| Mutex<SkipMap> | 191.2 / 469.5 / 1175.9 / 2689.5 / 4275.6 |

A scan over `ConcurrentSkipMap` copies a node's 32 entry pointers once and walks them; over `ArenaSkipMap` it follows nodes allocated in insertion order. crossbeam-skiplist follows one pointer per entry and pins the epoch for every one.

### Filling an empty map

Each thread inserts fresh distinct keys into an empty map, a new map every million entries.

| map | 1 / 2 / 4 / 8 / 14 threads |
|---|---|
| ConcurrentSkipMap | 325.8 / 337.6 / 419.6 / 528.0 / 927.7 |
| ArenaSkipMap | 268.1 / 284.4 / 351.6 / 577.6 / 1161.3 |
| crossbeam-skiplist | 398.5 / 527.6 / 685.2 / 1206.4 / 2174.2 |
| RwLock<BTreeMap> | 137.7 / 386.0 / 971.1 / 2100.6 / 3541.3 |
| Mutex<SkipMap> | 198.2 / 601.1 / 2121.6 / 5846.7 / 9606.9 |

## Methodology

The harness is `benches/concurrent.rs`. Every structure is wrapped in one small trait with `insert`, `get`, `remove` and a 100-entry range sum, so each competitor pays only for its own operations.

A cell prebuilds a map with the even keys below 200k (100k entries), starts the given number of worker threads behind a barrier, lets them run for the cell's time limit and sums the operations they completed. Each worker draws its keys from its own xorshift generator over `0..200k`, so half the lookups miss. The reported number is wall time times threads divided by operations: nanoseconds per operation per thread.

| workload | what one step does |
|---|---|
| read | `get` on a random key |
| read90_write10 | `get` with probability 0.9, otherwise `insert` or `remove` with equal probability |
| read50_write50 | the same with probability 0.5 |
| write | `insert` or `remove`, never `get` |
| range | a 100-entry scan from a random key; thread 0 writes instead when there are two or more threads |
| fill | inserts of distinct fresh keys into an empty map, restarting every million |

Thread counts are 1, 2, 4, 8 and the machine's core count. Cells run for 10 s and are not repeated, so a single cell carries the drift of its 10 s: on this machine that is 5 to 10% between runs an hour apart. Differences inside that band are not differences.

The machine is an Apple M4 Pro with 10 performance and 4 efficiency cores, in high-power mode for every run, with power mode and load recorded before and after each run. The 14-thread cells run on all cores including the efficiency cores, which is part of why every structure's 14-thread number steps up from its 8-thread one.

What these numbers do not say: nothing here is a mixed workload with scans and point writes at once, keys are 8-byte integers with 8-byte values and the map never exceeds a few hundred thousand entries, so every working set fits in the last-level cache. A workload with large keys, string comparison or a map in the tens of millions will move every number.

## Reproduction

```sh
cargo bench --bench concurrent -- --limit 10
```

That runs every competitor and both maps on every workload at 1, 2, 4, 8 and 14 threads, about half an hour. To narrow it:

```sh
# one impl, a few thread counts
cargo bench --bench concurrent -- --impls concurrent,crossbeam --threads 1,4,14

# one workload across every impl
cargo bench --bench concurrent -- --only range

# everything but the pure-read cells
cargo bench --bench concurrent -- --skip read

# a bigger map
cargo bench --bench concurrent -- --sizes 1000000
```

Impl names are `concurrent`, `arena`, `crossbeam`, `rwlock_btree` and `mutex_skipmap`. Output is one line per cell: impl, workload, threads, map size, operations completed and nanoseconds per operation per thread. Record your machine's power mode and load with the run; a number without them cannot be compared with another.
