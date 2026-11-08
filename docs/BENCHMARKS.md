# Single-threaded benchmarks

How `SkipMap`, `SkipSet`, `OrderedSkipList` and `SkipList` compare with `rust-skiplist` (the `skiplist` crate, 2.0.1) and with `std::collections::BTreeMap` and `BTreeSet`, how the numbers were taken and how to take them again.

## Contents

* [Results](#results)
  * [Map](#map)
  * [Set](#set)
  * [Ordered list](#ordered-list)
  * [Indexed list](#indexed-list)
* [Methodology](#methodology)
* [Reproduction](#reproduction)

## Results

Every cell is nanoseconds per step on a prebuilt structure of 1k or 100k elements, written `1k / 100k`. Lower is better. ◆ marks this crate's column. The fastest skip list at each size is bold; `std` is a yardstick and is not ranked. The gain columns are the other structure's time divided by ours, minus one: positive means ours is faster, negative slower. `rust-skiplist` is the `skiplist` crate, 2.0.1; `std` is `BTreeMap` or `BTreeSet`, which have no counterpart for the ordered list or the indexed list.

All runs are one pass of 10 s per cell, in high-power mode on an Apple M4 Pro.

### Map

| workload | xskiplist ns ◆ | rust-skiplist ns | vs rust-skiplist | std ns | vs std |
|---|---|---|---|---|---|
| map/insert_remove | **36.3** / **83.6** | 97.2 / 187.4 | +168% / +124% | 30.3 / 43.5 | -17% / -48% |
| map/insert_replace | **9.2** / **36.0** | 19.1 / 59.3 | +108% / +65% | 7.9 / 15.3 | -14% / -57% |
| map/get | **6.8** / **24.3** | 15.2 / 70.4 | +124% / +190% | 7.2 / 14.1 | +6% / -42% |
| map/get_mut | **8.4** / **28.6** | 15.5 / 49.6 | +85% / +73% | 7.3 / 14.6 | -13% / -49% |
| map/entry_hit | **9.1** / **35.2** | 18.0 / 58.0 | +98% / +65% | 8.1 / 14.8 | -11% / -58% |
| map/pop_last | **20.3** / **49.1** | 55.7 / 86.6 | +174% / +76% | 17.3 / 108.4 | -15% / +121% |
| map/iter | **340.3** / **49,607.8** | 4,523.3 / 2,116,333.9 | +1,229% / +4,166% | 599.1 / 72,782.4 | +76% / +47% |
| map/split_append | **70.4** / **100.8** | 117.0 / 252.7 | +66% / +151% | 5,970.1 / 620,150.3 | +8,380% / +615,128% |
| map/from_iter | **7,014.5** / **1,314,410.8** | 35,093.7 / 4,143,618.5 | +400% / +215% | 10,033.7 / 1,648,360.2 | +43% / +25% |
| map/retain | **6,539.9** / **706,410.4** | 33,166.7 / 4,250,418.5 | +407% / +502% | 8,186.1 / 823,351.3 | +25% / +17% |

### Set

| workload | xskiplist ns ◆ | rust-skiplist ns | vs rust-skiplist | std ns | vs std |
|---|---|---|---|---|---|
| set/insert_remove | **31.3** / **77.2** | 94.7 / 173.1 | +203% / +124% | 24.6 / 35.0 | -21% / -55% |
| set/insert_present | **8.8** / **31.5** | 19.3 / 60.2 | +119% / +91% | 7.9 / 13.1 | -10% / -58% |
| set/replace | **9.3** / **28.8** | 19.4 / 61.2 | +109% / +112% | 8.0 / 14.2 | -14% / -51% |
| set/entry_hit | **8.4** / **25.6** | 14.1 / 49.7 | +68% / +94% | 7.1 / 13.1 | -15% / -49% |
| set/contains | **6.5** / **24.0** | 15.6 / 76.6 | +140% / +219% | 7.1 / 13.2 | +9% / -45% |
| set/contains_string | **43.2** / **118.9** | 53.8 / 227.9 | +25% / +92% | 41.0 / 109.3 | -5% / -8% |
| set/insert_remove_string | **143.5** / **294.6** | 221.0 / 420.9 | +54% / +43% | 112.9 / 228.8 | -21% / -22% |
| set/range_next | **7.6** / **28.6** | 21.0 / 85.3 | +176% / +198% | 10.0 / 15.7 | +32% / -45% |
| set/pop_first_insert | **22.2** / **20.2** | 45.8 / 52.5 | +106% / +160% | 12.8 / 14.2 | -42% / -30% |
| set/iter | **292.1** / **42,099.6** | 4,795.8 / 2,162,478.3 | +1,542% / +5,037% | 1,880.7 / 192,682.3 | +544% / +358% |
| set/from_iter | **1,533.4** / **125,496.9** | 29,700.9 / 3,319,518.2 | +1,837% / +2,545% | 5,011.3 / 497,242.4 | +227% / +296% |
| set/from_iter_shuffled | **5,847.8** / **972,810.6** | 33,984.4 / 4,197,063.9 | +481% / +331% | 9,482.4 / 1,366,014.2 | +62% / +40% |
| set/clone | **976.1** / **78,016.5** | 30,142.8 / 11,066,520.2 | +2,988% / +14,085% | 5,931.8 / 609,882.3 | +508% / +682% |
| set/into_iter | **1,419.6** / **122,056.9** | 30,710.1 / 11,189,059.2 | +2,063% / +9,067% | 5,760.0 / 600,602.5 | +306% / +392% |
| set/bitor | **3,476.5** / **323,521.5** | 75,388.7 / 8,126,937.3 | +2,069% / +2,412% | 8,447.4 / 845,315.9 | +143% / +161% |
| set/intersection | **1,293.3** / **128,691.5** | 2,853.7 / 842,836.2 | +121% / +555% | 822.9 / 83,057.8 | -36% / -35% |
| set/split_append | **66.5** / **104.2** | 115.0 / 253.7 | +73% / +143% | 5,841.1 / 605,852.0 | +8,684% / +581,332% |
| set/retain | **4,880.9** / **547,011.0** | 33,193.8 / 4,912,115.3 | +580% / +798% | 8,656.0 / 840,732.7 | +77% / +54% |

### Ordered list

| workload | xskiplist ns ◆ | rust-skiplist ns | vs rust-skiplist | std ns | vs std |
|---|---|---|---|---|---|
| osl/insert_take | **29.9** / **80.7** | 93.9 / 178.1 | +214% / +121% | - / - | - / - |
| osl/insert_take_last | **36.6** / **75.9** | 99.1 / 200.9 | +171% / +165% | - / - | - / - |
| osl/contains | **7.0** / **25.1** | 15.6 / 76.3 | +123% / +204% | - / - | - / - |
| osl/rank | **7.1** / **29.1** | 18.5 / 79.9 | +161% / +175% | - / - | - / - |
| osl/get_by_index | **6.3** / **16.5** | 16.2 / 74.7 | +157% / +353% | - / - | - / - |
| osl/count_dups | **13.5** / **53.6** | 49.0 / 232.1 | +263% / +333% | - / - | - / - |
| osl/pop_first_insert | **19.4** / **22.5** | 45.6 / 51.9 | +135% / +131% | - / - | - / - |
| osl/iter | **294.1** / **40,400.5** | 4,830.0 / 2,304,200.0 | +1,542% / +5,603% | - / - | - / - |
| osl/range_iter | **61.3** / **132.8** | 399.9 / 1,585.3 | +552% / +1,094% | - / - | - / - |
| osl/split_append | **66.2** / **96.7** | 118.7 / 263.9 | +79% / +173% | - / - | - / - |
| osl/from_iter | **5,559.6** / **933,448.0** | 34,237.6 / 4,273,082.1 | +516% / +358% | - / - | - / - |
| osl/retain | **4,619.7** / **530,250.1** | 33,374.6 / 4,784,907.5 | +622% / +802% | - / - | - / - |

### Indexed list

| workload | xskiplist ns ◆ | rust-skiplist ns | vs rust-skiplist | std ns | vs std |
|---|---|---|---|---|---|
| list/get | **6.0** / **15.7** | 16.4 / 77.7 | +173% / +395% | - / - | - / - |
| list/insert_remove | **25.1** / **61.1** | 100.2 / 188.5 | +299% / +209% | - / - | - / - |
| list/queue | **34.8** / **43.7** | 64.1 / 74.6 | +84% / +71% | - / - | - / - |
| list/stack | **29.3** / **51.3** | 39.9 / 52.1 | +36% / +2% | - / - | - / - |
| list/push_front_pop_back | **30.7** / **39.4** | 61.1 / 68.4 | +99% / +74% | - / - | - / - |
| list/iter | **293.6** / **40,781.4** | 4,888.0 / 2,693,548.7 | +1,565% / +6,505% | - / - | - / - |
| list/clone | **984.1** / **80,245.1** | 29,673.0 / 12,614,922.4 | +2,915% / +15,620% | - / - | - / - |
| list/split_append | **65.2** / **89.7** | 86.3 / 170.2 | +32% / +90% | - / - | - / - |
| list/from_iter | **1,004.9** / **86,292.7** | 54,972.2 / 6,476,689.6 | +5,370% / +7,405% | - / - | - / - |
| list/retain | **4,572.4** / **522,010.8** | 58,015.3 / 8,208,492.1 | +1,169% / +1,472% | - / - | - / - |

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

`rust-skiplist` runs with its own geometric level generator (`Geometric::new_with_seed(16, 0.5, 42)`), as its own benchmarks do. The `std` column has no list or ordered-list cells because `std` has nothing to put there.

Cells run for 10 s and are not repeated. On this machine single-thread cells drift 5 to 10% between runs an hour apart with no code change, so a difference inside that band is not a difference. The machine's power mode and load were recorded before and after each run; one earlier run of this suite was discarded because load from another job was present when it started.

What these numbers do not say: keys are 8-byte integers (`set/*_string` uses short strings), the structures never exceed 100k elements, so every working set fits in cache. Each cell measures one operation in isolation rather than a mixed workload.

## Reproduction

```sh
cargo bench --bench prelim -- --limit 10
```

That runs ours and `rust-skiplist` on every cell at 1k and 100k, about 35 minutes. To narrow it:

```sh
# ours against std only
cargo bench --bench prelim -- --impls new,btreemap

# one structure
cargo bench --bench prelim -- --only map/

# one cell at one size
cargo bench --bench prelim -- --only map/get --sizes 100000
```

Impl names are `new` (this crate), `original` (`rust-skiplist`) and `btreemap` (`std`). Output is one line per cell: impl, workload, size, steps completed and nanoseconds per step. Record your machine's power mode and load with the run; a number without them cannot be compared with another.
