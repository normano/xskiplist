# xskiplist

[![Crates.io](https://img.shields.io/crates/v/xskiplist.svg)](https://crates.io/crates/xskiplist)
[![Docs.rs](https://docs.rs/xskiplist/badge.svg)](https://docs.rs/xskiplist)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)

Skiplist collections for Rust. `SkipMap` is a sorted map and `SkipSet` is a sorted set, with APIs that follow `BTreeMap` and `BTreeSet`. `OrderedSkipList` is a sorted list that keeps equal elements and answers rank and index queries. `SkipList` is a list with O(log n) access, insertion and removal at any index. Every structure splits and joins in O(log n). Its only dependency is `parking_lot`. The crate is experimental and its API may change between 0.x releases.

## Install

```toml
[dependencies]
xskiplist = "0.2"
```

The crate has no features.

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
