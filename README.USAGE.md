# Usage Guide: xskiplist

This guide shows how to use the four collections in `xskiplist`: `SkipMap`, `SkipSet`, `OrderedSkipList` and `SkipList`.

## Table of Contents

* [Core Concepts](#core-concepts)
* [Quick Start](#quick-start)
  * [A sorted map](#a-sorted-map)
  * [A sorted set](#a-sorted-set)
  * [An indexable list](#an-indexable-list)
* [Choosing a Structure](#choosing-a-structure)
* [Using the Sorted Map](#using-the-sorted-map)
  * [Inserting, reading and removing](#inserting-reading-and-removing)
  * [Working through an entry](#working-through-an-entry)
  * [Reading the ends](#reading-the-ends)
* [Using the Sorted Set](#using-the-sorted-set)
  * [Inserting and replacing](#inserting-and-replacing)
  * [Combining sets](#combining-sets)
* [Keeping Duplicates in Order](#keeping-duplicates-in-order)
  * [Counting and ranking](#counting-and-ranking)
  * [Removing one occurrence](#removing-one-occurrence)
* [Indexing a List](#indexing-a-list)
  * [Pushing and popping at the ends](#pushing-and-popping-at-the-ends)
  * [Inserting and removing by index](#inserting-and-removing-by-index)
* [Iterating and Ranging](#iterating-and-ranging)
* [Filtering in Place](#filtering-in-place)
* [Splitting and Joining](#splitting-and-joining)
* [Building from Iterators](#building-from-iterators)
* [What to Expect from Performance](#what-to-expect-from-performance)
* [Error Handling](#error-handling)

## Core Concepts

* **Skiplist**: a sorted linked list where each node also carries links that skip ahead over many nodes, so a search walks about log n links instead of n.
* **Tower**: the set of forward links one node carries. Its height is random, drawn with probability 1/4 per extra level, up to 24.
* **Link width**: every link records how many elements it skips, so every structure can find the element at a given index and report the index of a given element in O(log n).
* **Position**: the 0-based index of an element in order. `SkipList` and `OrderedSkipList` expose it; `SkipMap` and `SkipSet` use it internally for ranges.
* **Key**: the value a `SkipMap` orders by. The map keeps the key it was given first; `insert` on a present key replaces only the value.
* **Borrowed lookup**: lookups take `&Q` where the key type implements `Borrow<Q>`, so a `SkipMap<String, V>` is searched with a `&str`.
* **Entry**: a view of one key that is either `Vacant` or `Occupied`, so a lookup and the insert or update that follows it cost one search.
* **Range**: a sub-iterator over the elements whose keys fall within two bounds, built from `std::ops::RangeBounds`.
* **Split**: `split_off` cuts a structure in two at a key or an index in O(log n). Both halves are independent structures.
* **Append**: `append` moves every element of one structure to the end of another in O(log n) when the two are already in order. Otherwise it merges them.
* **Lineage**: the structures that came out of one another through splits and appends. They share the memory their nodes live in, which is released when the last of them drops.
* **Stable duplicates**: `OrderedSkipList` keeps equal elements in the order they were inserted. `take` removes the earliest, `take_last` the latest.

## Quick Start

### A sorted map

```rust
use xskiplist::SkipMap;

fn main() {
  let mut ages: SkipMap<&str, u32> = SkipMap::new();
  ages.insert("mara", 41);
  ages.insert("ivo", 29);
  ages.insert("sefu", 35);

  assert_eq!(ages.get("ivo"), Some(&29));
  assert_eq!(ages.first_key_value(), Some((&"ivo", &29)));

  *ages.entry("sefu").or_insert(0) += 1;
  assert_eq!(ages["sefu"], 36);

  let under_forty: Vec<&str> = ages.iter().filter(|(_, age)| **age < 40).map(|(name, _)| *name).collect();
  assert_eq!(under_forty, ["ivo", "sefu"]);
}
```

### A sorted set

```rust
use xskiplist::SkipSet;

fn main() {
  let evens: SkipSet<u32> = (0..10).filter(|n| n % 2 == 0).collect();
  let threes: SkipSet<u32> = (0..10).filter(|n| n % 3 == 0).collect();

  let both: Vec<u32> = evens.intersection(&threes).copied().collect();
  assert_eq!(both, [0, 6]);

  let either = &evens | &threes;
  assert_eq!(either.len(), 7);
  assert_eq!(either.range(4..).next(), Some(&4));
}
```

### An indexable list

```rust
use xskiplist::SkipList;

fn main() {
  let mut queue: SkipList<&str> = SkipList::new();
  queue.push_back("b");
  queue.push_back("d");
  queue.insert(1, "c");
  queue.push_front("a");

  assert_eq!(queue.get(2), Some(&"c"));
  assert_eq!(queue.remove(1), "b");
  assert_eq!(queue.pop_front(), Some("a"));
  assert_eq!(queue.iter().copied().collect::<Vec<_>>(), ["c", "d"]);
}
```

## Choosing a Structure

| You need | Use |
|---|---|
| Keys to values, each key once, in key order | `SkipMap<K, V>` |
| Values once each, in order, with set algebra | `SkipSet<T>` |
| Values in order, equal values kept, rank and count queries | `OrderedSkipList<T>` |
| Values in the order you put them, addressed by index | `SkipList<T>` |

`SkipMap` and `SkipSet` follow the API of `BTreeMap` and `BTreeSet`. `SkipList` follows `VecDeque` where the operations match. `OrderedSkipList` has no standard counterpart.

## Using the Sorted Map

### Inserting, reading and removing

`insert` returns the old value when the key was present. Lookups accept any borrowed form of the key.

```rust
use xskiplist::SkipMap;

let mut prices: SkipMap<String, u64> = SkipMap::new();
assert_eq!(prices.insert("tea".to_string(), 300), None);
assert_eq!(prices.insert("tea".to_string(), 320), Some(300));

assert_eq!(prices.get("tea"), Some(&320));
assert!(prices.contains_key("tea"));
if let Some(price) = prices.get_mut("tea") {
  *price -= 20;
}
assert_eq!(prices.remove("tea"), Some(300));
assert_eq!(prices.remove("tea"), None);
```

`remove_entry` returns the stored key with the value. `get_key_value` reads both without removing.

### Working through an entry

`entry` searches once and hands back a `Vacant` or `Occupied` view. Use it when the next step depends on whether the key is there.

```rust
use xskiplist::SkipMap;
use xskiplist::map::Entry;

let mut counts: SkipMap<char, usize> = SkipMap::new();
for c in "mississippi".chars() {
  *counts.entry(c).or_insert(0) += 1;
}
assert_eq!(counts[&'s'], 4);

match counts.entry('m') {
  Entry::Occupied(e) if *e.get() == 1 => {
    e.remove();
  }
  Entry::Occupied(mut e) => *e.get_mut() = 0,
  Entry::Vacant(e) => {
    e.insert(0);
  }
}
assert!(!counts.contains_key(&'m'));

counts.entry('z').and_modify(|n| *n += 1).or_default();
assert_eq!(counts[&'z'], 0);
```

### Reading the ends

```rust
use xskiplist::SkipMap;

let mut log: SkipMap<u64, &str> = SkipMap::from([(3, "c"), (1, "a"), (2, "b")]);
assert_eq!(log.first_key_value(), Some((&1, &"a")));
assert_eq!(log.last_key_value(), Some((&3, &"c")));
assert_eq!(log.pop_first(), Some((1, "a")));
assert_eq!(log.pop_last(), Some((3, "c")));
assert_eq!(log.len(), 1);
```

## Using the Sorted Set

### Inserting and replacing

`insert` keeps the stored value when an equal one is present and returns `false`. `replace` swaps in the new value and returns the old one. `take` removes and returns the stored value.

```rust
use xskiplist::SkipSet;

let mut names: SkipSet<String> = SkipSet::new();
assert!(names.insert("Ada".to_string()));
assert!(!names.insert("Ada".to_string()));
assert_eq!(names.replace("Ada".to_string()), Some("Ada".to_string()));
assert_eq!(names.get("Ada").map(String::as_str), Some("Ada"));
assert_eq!(names.take("Ada"), Some("Ada".to_string()));
assert!(names.is_empty());
```

`entry` works as on the map, with `Vacant::insert` returning a reference to the stored value.

### Combining sets

The four methods return lazy iterators over borrowed values in order. The four operators build a new set.

```rust
use xskiplist::SkipSet;

let a: SkipSet<u32> = [1, 2, 3, 4].into();
let b: SkipSet<u32> = [3, 4, 5].into();

assert_eq!(a.union(&b).count(), 5);
assert_eq!(a.intersection(&b).copied().collect::<Vec<_>>(), [3, 4]);
assert_eq!(a.difference(&b).copied().collect::<Vec<_>>(), [1, 2]);
assert_eq!(a.symmetric_difference(&b).copied().collect::<Vec<_>>(), [1, 2, 5]);

let union: SkipSet<u32> = &a | &b;
let common: SkipSet<u32> = &a & &b;
let only_a: SkipSet<u32> = &a - &b;
let one_side: SkipSet<u32> = &a ^ &b;
assert_eq!((union.len(), common.len(), only_a.len(), one_side.len()), (5, 2, 2, 3));
```

## Keeping Duplicates in Order

`OrderedSkipList` stores every value it is given, equal values included. Equal values stay in insertion order.

### Counting and ranking

```rust
use xskiplist::OrderedSkipList;

let mut scores: OrderedSkipList<u32> = OrderedSkipList::new();
for s in [70, 85, 70, 92, 85, 70] {
  scores.insert(s);
}
assert_eq!(scores.iter().copied().collect::<Vec<_>>(), [70, 70, 70, 85, 85, 92]);

assert_eq!(scores.count(&70), 3);
assert_eq!(scores.rank(&85), Some(3));
assert_eq!(scores.rank(&80), None);
assert_eq!(scores.get_by_index(4), Some(&85));
assert_eq!(scores.first(), Some(&70));
assert_eq!(scores.last(), Some(&92));
```

`rank` is the index of the first equal element, so `rank` and `count` together give the span of a value.

### Removing one occurrence

```rust
use xskiplist::OrderedSkipList;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
struct Job { priority: u8, id: u32 }

impl PartialEq for Job {
  fn eq(&self, other: &Self) -> bool { self.priority == other.priority }
}
impl Eq for Job {}
impl PartialOrd for Job {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for Job {
  fn cmp(&self, other: &Self) -> Ordering { self.priority.cmp(&other.priority) }
}

let mut jobs: OrderedSkipList<Job> = OrderedSkipList::new();
jobs.insert(Job { priority: 1, id: 10 });
jobs.insert(Job { priority: 1, id: 11 });
jobs.insert(Job { priority: 2, id: 12 });

let probe = Job { priority: 1, id: 0 };
assert_eq!(jobs.take(&probe).map(|j| j.id), Some(10));
assert_eq!(jobs.take_last(&probe).map(|j| j.id), Some(11));
assert_eq!(jobs.remove(0).id, 12);
assert!(jobs.is_empty());
```

`take` removes the earliest equal element and `take_last` the latest. Equality must agree with the ordering, as `Ord` requires, since equal elements are found by comparing. `remove(index)` removes by position and panics when the index is out of range.

## Indexing a List

### Pushing and popping at the ends

```rust
use xskiplist::SkipList;

let mut window: SkipList<f64> = SkipList::new();
for sample in [1.0, 2.0, 3.0, 4.0] {
  window.push_back(sample);
  if window.len() > 3 {
    window.pop_front();
  }
}
assert_eq!(window.front(), Some(&2.0));
assert_eq!(window.back(), Some(&4.0));
*window.back_mut().unwrap() = 5.0;
assert_eq!(window.pop_back(), Some(5.0));
```

### Inserting and removing by index

```rust
use xskiplist::SkipList;

let mut line: SkipList<char> = "acd".chars().collect();
line.insert(1, 'b');
assert_eq!(line.iter().collect::<String>(), "abcd");
assert_eq!(line.remove(2), 'c');
assert_eq!(line[2], 'd');
line[0] = 'A';
assert_eq!(line.get(0), Some(&'A'));
assert_eq!(line.get(3), None);
```

`insert` accepts an index up to `len`, `remove` an index below `len`. Both are O(log n) at any position.

## Iterating and Ranging

Every structure iterates in order from both ends and reports its exact length. `SkipMap` and `SkipList` also iterate mutably.

```rust
use xskiplist::{SkipMap, SkipSet, OrderedSkipList};
use std::ops::Bound;

let map: SkipMap<u32, &str> = (1..=5).map(|k| (k, "x")).collect();
assert_eq!(map.keys().rev().next(), Some(&5));
assert_eq!(map.range(2..4).count(), 2);
assert_eq!(map.range((Bound::Excluded(2), Bound::Unbounded)).next(), Some((&3, &"x")));
assert_eq!(map.range(..).len(), 5);

let set: SkipSet<u32> = (1..=5).collect();
assert_eq!(set.range(3..).copied().collect::<Vec<_>>(), [3, 4, 5]);

let dups: OrderedSkipList<u32> = [1, 2, 2, 3].into();
assert_eq!(dups.range(2..3).count(), 2);
```

A range borrowed by `&str` on a `SkipMap<String, V>` needs the borrowed type named: `map.range::<str, _>("a".."m")`.

## Filtering in Place

`retain` visits every element in order and drops those the closure rejects. The structure stays consistent if the closure panics; the elements already visited keep the outcome they got.

```rust
use xskiplist::{SkipMap, SkipList};

let mut stock: SkipMap<&str, u32> = SkipMap::from([("nut", 0), ("bolt", 12), ("washer", 3)]);
stock.retain(|_, qty| {
  *qty = qty.saturating_sub(1);
  *qty > 0
});
assert_eq!(stock.len(), 2);

let mut list: SkipList<u32> = (0..10).collect();
list.retain(|n| n % 3 == 0);
assert_eq!(list.iter().copied().collect::<Vec<_>>(), [0, 3, 6, 9]);
```

## Splitting and Joining

`split_off` cuts in O(log n). `append` joins in O(log n) when every element of the second structure comes after every element of the first; otherwise the map, the set and the ordered list merge the two in O(n + m). The plain list always joins in O(log n).

```rust
use xskiplist::{SkipMap, SkipList, OrderedSkipList};

let mut map: SkipMap<u32, &str> = (0..6).map(|k| (k, "a")).collect();
let mut high = map.split_off(&3);
assert_eq!(map.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
assert_eq!(high.keys().copied().collect::<Vec<_>>(), [3, 4, 5]);

high.insert(1, "b");
map.append(&mut high);
assert!(high.is_empty());
assert_eq!(map[&1], "b");

let mut list: SkipList<u32> = (0..6).collect();
let mut tail = list.split_off(2);
list.append(&mut tail);
assert_eq!(list.len(), 6);

let mut dups: OrderedSkipList<u32> = [1, 3, 3].into();
let mut more: OrderedSkipList<u32> = [2, 3].into();
dups.append(&mut more);
assert_eq!(dups.iter().copied().collect::<Vec<_>>(), [1, 2, 3, 3, 3]);
```

When the map merges, a key present on both sides keeps the value from the appended map, as `BTreeMap::append` does. When the ordered list merges, equal elements from the receiving list stay ahead of those from the appended one. `OrderedSkipList::split_off_index` splits by position instead of by value.

Structures that have been split from or appended to one another share the memory their nodes live in. That memory is released when the last structure of the lineage drops, so a small structure split from a large one keeps the large one's node memory alive until it is dropped or cleared.

## Building from Iterators

Collecting builds the structure in one pass over the sorted input. For the map and the set the input is sorted first and a key given several times keeps its last value, as `BTreeMap` does. The plain list keeps the input order.

```rust
use xskiplist::{SkipMap, SkipSet, SkipList};

let map: SkipMap<u32, &str> = [(2, "b"), (1, "a"), (2, "c")].into_iter().collect();
assert_eq!(map[&2], "c");

let set: SkipSet<u32> = "3141592".chars().map(|c| c.to_digit(10).unwrap()).collect();
assert_eq!(set.iter().copied().collect::<Vec<_>>(), [1, 2, 3, 4, 5, 9]);

let list: SkipList<u32> = [3, 1, 2].into();
assert_eq!(list.iter().copied().collect::<Vec<_>>(), [3, 1, 2]);

let mut grown = map.clone();
grown.extend([(3, "d")]);
assert_eq!(grown.len(), 3);
```

## What to Expect from Performance

* Lookup, insert, remove, index access, rank, split and append are O(log n) on average. Iteration is O(n). Towers are drawn with probability 1/4 per level, capped at 24, so a structure of a million elements has a height around ten.
* Nodes live in per-height slabs owned by the structure, so inserts and removes do not call the global allocator once a slab has grown. Clearing or dropping a structure frees memory in large blocks.
* A node costs its key and value plus three words of header plus two words per level of its tower, averaging about four words of links.
* Iteration and range scans follow pointers from node to node. On large structures this is several times slower than scanning a `BTreeMap`, whose nodes hold many keys each. Point operations on large structures are within a small factor of `BTreeMap`.
* `SkipList::push_back`, `pop_back` and `insert` near the end descend from the head like any other position.

## Error Handling

No method returns a `Result`. The ways a call can fail are reported as panics. Each is a caller error:

```rust
use xskiplist::{SkipList, SkipMap};
use std::ops::Bound;

let mut list: SkipList<u32> = (0..3).collect();
let map: SkipMap<u32, u32> = SkipMap::new();

// list.insert(4, 0);                     // index 4 > len 3
// list.remove(3);                        // index 3 >= len 3
// list.split_off(4);                     // index 4 > len 3
// let _ = list[3];                       // Index out of bounds
// let _ = map[&7];                       // key not present
// map.range(5..2);                       // start greater than end
// map.range((Bound::Excluded(2), Bound::Excluded(2)));  // both bounds equal and excluded
list.insert(3, 0);
assert_eq!(list.len(), 4);
```

Lookups with `get`, `get_by_index`, `remove` on a map or set and `take` return `Option` instead of panicking.
