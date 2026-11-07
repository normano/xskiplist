# Single-threaded benchmarks

How `SkipMap`, `SkipSet`, `OrderedSkipList` and `SkipList` compare with the `skiplist` crate they replace and with `std::collections::BTreeMap` and `BTreeSet`, how the numbers were taken and how to take them again.

## Contents

* [Results](#results)
  * [Map](#map)
  * [Set](#set)
  * [Ordered list](#ordered-list)
  * [Indexed list](#indexed-list)
* [Methodology](#methodology)
* [Reproduction](#reproduction)

## Results

Every cell is nanoseconds per step on a prebuilt structure of 1k or 100k elements, written `1k / 100k`. Lower is better. The gain columns are the other structure's time divided by ours, minus one: positive means ours is faster, negative slower. `original` is `skiplist` 2.0.1, the crate these structures replace; `std` is `BTreeMap` or `BTreeSet`, which have no counterpart for the ordered list or the indexed list.

All runs were taken on 2026-11-07, one pass of 10 s per cell, in high-power mode on an Apple M4 Pro.

### Map

| workload | original ns | xskiplist ns | std ns | vs original | vs std |
|---|---|---|---|---|---|
| map/insert_remove | 97.2 / 187.4 | 36.3 / 83.6 | 30.3 / 43.5 | +168% / +124% | -17% / -48% |
| map/insert_replace | 19.1 / 59.3 | 9.2 / 36.0 | 7.9 / 15.3 | +108% / +65% | -14% / -57% |
| map/get | 15.2 / 70.4 | 6.8 / 24.3 | 7.2 / 14.1 | +124% / +190% | +6% / -42% |
| map/get_mut | 15.5 / 49.6 | 8.4 / 28.6 | 7.3 / 14.6 | +85% / +73% | -13% / -49% |
| map/entry_hit | 18.0 / 58.0 | 9.1 / 35.2 | 8.1 / 14.8 | +98% / +65% | -11% / -58% |
| map/pop_last | 55.7 / 86.6 | 20.3 / 49.1 | 17.3 / 108.4 | +174% / +76% | -15% / +121% |
| map/iter | 4523.3 / 2116333.9 | 340.3 / 49607.8 | 599.1 / 72782.4 | +1229% / +4166% | +76% / +47% |
| map/split_append | 117.0 / 252.7 | 70.4 / 100.8 | 5970.1 / 620150.3 | +66% / +151% | +8380% / +615128% |
| map/from_iter | 35093.7 / 4143618.5 | 7014.5 / 1314410.8 | 10033.7 / 1648360.2 | +400% / +215% | +43% / +25% |
| map/retain | 33166.7 / 4250418.5 | 6539.9 / 706410.4 | 8186.1 / 823351.3 | +407% / +502% | +25% / +17% |

### Set

| workload | original ns | xskiplist ns | std ns | vs original | vs std |
|---|---|---|---|---|---|
| set/insert_remove | 94.7 / 173.1 | 31.3 / 77.2 | 24.6 / 35.0 | +203% / +124% | -21% / -55% |
| set/insert_present | 19.3 / 60.2 | 8.8 / 31.5 | 7.9 / 13.1 | +119% / +91% | -10% / -58% |
| set/replace | 19.4 / 61.2 | 9.3 / 28.8 | 8.0 / 14.2 | +109% / +112% | -14% / -51% |
| set/entry_hit | 14.1 / 49.7 | 8.4 / 25.6 | 7.1 / 13.1 | +68% / +94% | -15% / -49% |
| set/contains | 15.6 / 76.6 | 6.5 / 24.0 | 7.1 / 13.2 | +140% / +219% | +9% / -45% |
| set/contains_string | 53.8 / 227.9 | 43.2 / 118.9 | 41.0 / 109.3 | +25% / +92% | -5% / -8% |
| set/insert_remove_string | 221.0 / 420.9 | 143.5 / 294.6 | 112.9 / 228.8 | +54% / +43% | -21% / -22% |
| set/range_next | 21.0 / 85.3 | 7.6 / 28.6 | 10.0 / 15.7 | +176% / +198% | +32% / -45% |
| set/pop_first_insert | 45.8 / 52.5 | 22.2 / 20.2 | 12.8 / 14.2 | +106% / +160% | -42% / -30% |
| set/iter | 4795.8 / 2162478.3 | 292.1 / 42099.6 | 1880.7 / 192682.3 | +1542% / +5037% | +544% / +358% |
| set/from_iter | 29700.9 / 3319518.2 | 1533.4 / 125496.9 | 5011.3 / 497242.4 | +1837% / +2545% | +227% / +296% |
| set/from_iter_shuffled | 33984.4 / 4197063.9 | 5847.8 / 972810.6 | 9482.4 / 1366014.2 | +481% / +331% | +62% / +40% |
| set/clone | 30142.8 / 11066520.2 | 976.1 / 78016.5 | 5931.8 / 609882.3 | +2988% / +14085% | +508% / +682% |
| set/into_iter | 30710.1 / 11189059.2 | 1419.6 / 122056.9 | 5760.0 / 600602.5 | +2063% / +9067% | +306% / +392% |
| set/bitor | 75388.7 / 8126937.3 | 3476.5 / 323521.5 | 8447.4 / 845315.9 | +2069% / +2412% | +143% / +161% |
| set/intersection | 2853.7 / 842836.2 | 1293.3 / 128691.5 | 822.9 / 83057.8 | +121% / +555% | -36% / -35% |
| set/split_append | 115.0 / 253.7 | 66.5 / 104.2 | 5841.1 / 605852.0 | +73% / +143% | +8684% / +581332% |
| set/retain | 33193.8 / 4912115.3 | 4880.9 / 547011.0 | 8656.0 / 840732.7 | +580% / +798% | +77% / +54% |

### Ordered list

| workload | original ns | xskiplist ns | std ns | vs original | vs std |
|---|---|---|---|---|---|
| osl/insert_take | 93.9 / 178.1 | 29.9 / 80.7 | - / - | +214% / +121% | - / - |
| osl/insert_take_last | 99.1 / 200.9 | 36.6 / 75.9 | - / - | +171% / +165% | - / - |
| osl/contains | 15.6 / 76.3 | 7.0 / 25.1 | - / - | +123% / +204% | - / - |
| osl/rank | 18.5 / 79.9 | 7.1 / 29.1 | - / - | +161% / +175% | - / - |
| osl/get_by_index | 16.2 / 74.7 | 6.3 / 16.5 | - / - | +157% / +353% | - / - |
| osl/count_dups | 49.0 / 232.1 | 13.5 / 53.6 | - / - | +263% / +333% | - / - |
| osl/pop_first_insert | 45.6 / 51.9 | 19.4 / 22.5 | - / - | +135% / +131% | - / - |
| osl/iter | 4830.0 / 2304200.0 | 294.1 / 40400.5 | - / - | +1542% / +5603% | - / - |
| osl/range_iter | 399.9 / 1585.3 | 61.3 / 132.8 | - / - | +552% / +1094% | - / - |
| osl/split_append | 118.7 / 263.9 | 66.2 / 96.7 | - / - | +79% / +173% | - / - |
| osl/from_iter | 34237.6 / 4273082.1 | 5559.6 / 933448.0 | - / - | +516% / +358% | - / - |
| osl/retain | 33374.6 / 4784907.5 | 4619.7 / 530250.1 | - / - | +622% / +802% | - / - |

### Indexed list

| workload | original ns | xskiplist ns | std ns | vs original | vs std |
|---|---|---|---|---|---|
| list/get | 16.4 / 77.7 | 6.0 / 15.7 | - / - | +173% / +395% | - / - |
| list/insert_remove | 100.2 / 188.5 | 25.1 / 61.1 | - / - | +299% / +209% | - / - |
| list/queue | 64.1 / 74.6 | 34.8 / 43.7 | - / - | +84% / +71% | - / - |
| list/stack | 39.9 / 52.1 | 29.3 / 51.3 | - / - | +36% / +2% | - / - |
| list/push_front_pop_back | 61.1 / 68.4 | 30.7 / 39.4 | - / - | +99% / +74% | - / - |
| list/iter | 4888.0 / 2693548.7 | 293.6 / 40781.4 | - / - | +1565% / +6505% | - / - |
| list/clone | 29673.0 / 12614922.4 | 984.1 / 80245.1 | - / - | +2915% / +15620% | - / - |
| list/split_append | 86.3 / 170.2 | 65.2 / 89.7 | - / - | +32% / +90% | - / - |
| list/from_iter | 54972.2 / 6476689.6 | 1004.9 / 86292.7 | - / - | +5370% / +7405% | - / - |
| list/retain | 58015.3 / 8208492.1 | 4572.4 / 522010.8 | - / - | +1169% / +1472% | - / - |

## Methodology

The harness is `benches/prelim.rs`. Every structure is wrapped in one small trait per shape (list, ordered list, set, map), so each competitor pays only for its own operation.

A cell prebuilds the structure, then runs one step in a loop for the time limit and reports the time per step. Maps and sets hold the even keys `0, 2, ..., 2(n - 1)`, so a lookup of a random even key hits and an insert of an odd key lands at a random position between two present keys. Lists hold `0..n` and are addressed by index. Keys come from a fixed seeded pool of 1024, so every structure sees the same sequence. `osl/count_dups` holds `i / 100` for `i` in `0..n`, a hundred copies of each value.

| step | what it does |
|---|---|
| `get`, `contains`, `rank`, `get_by_index` | one lookup of a present key or index |
| `insert_remove`, `insert_take` | insert an absent key, then remove it |
| `insert_present`, `insert_replace`, `replace` | insert a present key |
| `entry_hit` | the `entry` API on a present key |
| `pop_first_insert`, `pop_last` | take one end, then put the key back |
| `queue`, `stack`, `push_front_pop_back` | one push and one pop at the named ends |
| `range_next`, `range_iter` | build a range from a random key and read its first element or its first 100 elements |
| `split_append` | split at a random key, then join the halves again |
| `iter`, `clone`, `into_iter`, `from_iter`, `retain`, `bitor`, `intersection` | one pass over the whole structure |

The `original` crate runs with its own geometric level generator (`Geometric::new_with_seed(16, 0.5, 42)`), as its own benchmarks do. The `std` column has no list or ordered-list cells because `std` has nothing to put there.

Cells run for 10 s and are not repeated. On this machine single-thread cells drift 5 to 10% between runs an hour apart with no code change, so a difference inside that band is not a difference. The machine's power mode and load were recorded before and after each run; one earlier run of this suite was discarded because load from another job was present when it started.

What these numbers do not say: keys are 8-byte integers (`set/*_string` uses short strings), the structures never exceed 100k elements, so every working set fits in cache. Each cell measures one operation in isolation rather than a mixed workload.

## Reproduction

```sh
cargo bench --bench prelim -- --limit 10
```

That runs ours and the original on every cell at 1k and 100k, about 35 minutes. To narrow it:

```sh
# ours against std only
cargo bench --bench prelim -- --impls new,btreemap

# one structure
cargo bench --bench prelim -- --only map/

# one cell at one size
cargo bench --bench prelim -- --only map/get --sizes 100000
```

Impl names are `new` (this crate), `original` (`skiplist` 2.0.1) and `btreemap` (`std`). Output is one line per cell: impl, workload, size, steps completed and nanoseconds per step. Record your machine's power mode and load with the run; a number without them cannot be compared with another.
