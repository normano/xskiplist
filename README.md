# XSkipList

[![Crates.io](https://img.shields.io/crates/v/xskiplist.svg)](https://crates.io/crates/xskiplist)
[![Docs.rs](https://docs.rs/xskiplist/badge.svg)](https://docs.rs/xskiplist)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)

High Performance Skip List collections for Rust. Every structure splits and joins in O(log n).

`SkipMap` is a sorted map and `SkipSet` is a sorted set, with APIs that follow `BTreeMap` and `BTreeSet`. 

`OrderedSkipList` is a sorted list that keeps equal elements and answers rank and index queries. 

`SkipList` is a list with O(log n) access, insertion and removal at any index. 

`ConcurrentSkipMap` is a sorted map shared between threads with no locks on the read path. `ArenaSkipMap` is its insert-only sibling, which hands out plain references and never frees before it drops.

## Performance highlights

* Nodes hold up to 32 elements side by side, so scans read cache lines instead of chasing pointers.
* Iterating a `SkipMap` of 100k entries is 1.5x faster than `BTreeMap`.
* Splitting a `SkipMap` of 100k entries and joining the halves again costs 100 ns. `BTreeMap` takes 620 µs.
* Faster than the `skiplist` crate on every one of 100 measured cells, most by 2x to 5x.
* Point lookups and updates on a map of 100k entries take 1.7x to 2.4x longer than on `BTreeMap`. Scans, splits and whole-structure operations are faster.
* `ConcurrentSkipMap` beats `crossbeam-skiplist` on every measured cell: reads by 4% to 15%, writes by 1.9x to 3.2x, range scans by 1.6x to 4.6x on 14 cores.

Full results, method and the commands to repeat them: [docs/BENCHMARKS.md](https://github.com/normano/xskiplist/blob/main/docs/BENCHMARKS.md) and [docs/CONCURRENT_BENCHMARKS.md](https://github.com/normano/xskiplist/blob/main/docs/CONCURRENT_BENCHMARKS.md).

## Install

```toml
[dependencies]
xskiplist = "0.9"
```

## Common tasks

| To do this | Use |
|---|---|
| Keep key-value pairs sorted by key | `SkipMap::insert`, `SkipMap::get`, `SkipMap::remove`, `SkipMap::entry` |
| Keep a sorted set of values | `SkipSet::insert`, `SkipSet::contains`, `SkipSet::remove`, `SkipSet::replace` |
| Keep sorted values with duplicates | `OrderedSkipList::insert`, `OrderedSkipList::take`, `OrderedSkipList::count` |
| Find a value's position in sorted order | `OrderedSkipList::rank`, `OrderedSkipList::get_by_index` |
| Insert, remove or read by index | `SkipList::insert`, `SkipList::remove`, `SkipList::get` |
| Use a list as a queue or stack | `SkipList::push_back`, `SkipList::pop_front`, `SkipList::pop_back` |
| Visit entries in a key range | `SkipMap::range`, `SkipMap::range_mut`, `SkipSet::range`, `OrderedSkipList::range` |
| Take the smallest or largest entry | `pop_first`, `pop_last`, `first_key_value`, `last_key_value` |
| Split a structure in two or join two | `split_off`, `append` on every structure |
| Combine two sets | `SkipSet::union`, `SkipSet::intersection`, `&a \| &b`, `&a & &b` |
| Drop entries that fail a test | `retain` on every structure |
| Build from an iterator | `collect::<SkipMap<_, _>>()`, `collect::<SkipSet<_>>()`, `collect::<SkipList<_>>()` |
| Share a sorted map between threads | `ConcurrentSkipMap::insert`, `ConcurrentSkipMap::get`, `ConcurrentSkipMap::remove`, `ConcurrentSkipMap::range` |
| Fill a map from many threads and read it with plain references | `ArenaSkipMap::insert`, `ArenaSkipMap::get_or_insert`, `ArenaSkipMap::get`, `ArenaSkipMap::range` |

## Benchmarks

Nanoseconds per operation, lower is better. One pass of 10 s per cell on an Apple M4 Pro in high-power mode, 2026-11-07. 
* The gain is the other structure's time divided by ours, minus one. ◆ marks this crate's structures. 
* The fastest skip list at each size or thread count is bold; `std` is a yardstick and is not ranked.

### Single-threaded

Structures of 100k elements; `rust-skiplist` is `skiplist` 2.0.1 on crates.io and `std` is `BTreeMap` or `BTreeSet`.

| operation | xskiplist ns ◆ | rust-skiplist ns | vs rust-skiplist | std ns | vs std |
|---|---|---|---|---|---|
| map lookup | **24.3** | 70.4 | +190% | 14.1 | -42% |
| map insert and remove | **83.6** | 187.4 | +124% | 43.5 | -48% |
| map iterate, whole structure | **49,607.8** | 2,116,333.9 | +4,166% | 72,782.4 | +47% |
| map split and join | **100.8** | 252.7 | +151% | 620,150.3 | +615,128% |
| set lookup | **24.0** | 76.6 | +219% | 13.2 | -45% |
| set iterate, whole structure | **42,099.6** | 2,162,478.3 | +5,037% | 192,682.3 | +358% |
| ordered list rank | **29.1** | 79.9 | +175% | - | - |
| list get by index | **15.7** | 77.7 | +395% | - | - |
| list insert and remove by index | **61.1** | 188.5 | +209% | - | - |

### Multi-threaded

Maps of 100k entries, half of the lookups missing; cells hold 1 / 4 / 14 threads on 14 cores. The competitor is `crossbeam-skiplist` 0.1. 
* `ArenaSkipMap` cannot remove, so it has no cells in the mixed and write-only workloads.

| workload | ConcurrentSkipMap ns ◆ | ArenaSkipMap ns ◆ | crossbeam ns | Concurrent vs crossbeam | Arena vs crossbeam |
|---|---|---|---|---|---|
| point reads, half missing | **116.7** / **122.9** / 160.4 | 126.1 / 128.6 / **158.2** | 121.6 / 139.0 / 184.1 | +4% / +13% / +15% | -4% / +8% / +16% |
| 90% reads, 10% writes | **135.1** / **150.3** / **194.5** | - | 179.5 / 210.9 / 335.2 | +33% / +40% / +72% | - |
| 50% reads, 50% writes | **146.8** / **196.3** / **283.8** | - | 235.7 / 348.7 / 760.4 | +61% / +78% / +168% | - |
| writes only | **157.5** / **205.0** / **401.3** | - | 298.2 / 527.2 / 1,301.9 | +89% / +157% / +224% | - |
| 100-entry range scan under writes | 501.4 / 435.1 / 891.7 | **253.4** / **151.7** / **259.2** | 777.4 / 1,004.9 / 4,087.7 | +55% / +131% / +358% | +207% / +562% / +1,477% |
| fill an empty map | 325.8 / 419.6 / **927.7** | **268.1** / **351.6** / 1,161.3 | 398.5 / 685.2 / 2,174.2 | +22% / +63% / +134% | +49% / +95% / +87% |

## Verification

The concurrent maps pass seven tiers: the unit suite, release stress gates on every core, shuttle's randomised and PCT schedules, exhaustive loom models of the split, unlink and same-key protocols, a Miri seed sweep, ThreadSanitizer and written readings of the orderings no checker models. The tiers, what each one cannot see and how to run them are in [verify/GUIDE.md](https://github.com/normano/xskiplist/blob/main/verify/GUIDE.md).

## Documentation

* [Usage guide](https://github.com/normano/xskiplist/blob/main/README.USAGE.md): how to do things, in code.
* [API reference](https://github.com/normano/xskiplist/blob/main/API_REFERENCE.md): every type and signature.
* [docs.rs](https://docs.rs/xskiplist): the guide and the rustdoc.
