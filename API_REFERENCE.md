# API Reference: xskiplist

Skiplist collections: a sorted map, a sorted set, a sorted list that keeps duplicates and a list indexed by position.

## Contents

* [1. SkipMap](#1-skipmap)
  * [SkipMap<K, V>](#skipmapk-v)
  * [map::Entry<'a, K, V>](#mapentrya-k-v)
  * [map::VacantEntry<'a, K, V>](#mapvacantentrya-k-v)
  * [map::OccupiedEntry<'a, K, V>](#mapoccupiedentrya-k-v)
  * [Map iterators](#map-iterators)
* [2. SkipSet](#2-skipset)
  * [SkipSet<T>](#skipsett)
  * [set::Entry<'a, T>](#setentrya-t)
  * [set::VacantEntry<'a, T>](#setvacantentrya-t)
  * [set::OccupiedEntry<'a, T>](#setoccupiedentrya-t)
  * [Set iterators](#set-iterators)
* [3. OrderedSkipList](#3-orderedskiplist)
  * [OrderedSkipList<T>](#orderedskiplistt)
  * [Ordered list iterators](#ordered-list-iterators)
* [4. SkipList](#4-skiplist)
  * [SkipList<T>](#skiplistt)
  * [List iterators](#list-iterators)
* [5. Shared Behaviour](#5-shared-behaviour)
  * [Complexity](#complexity)
  * [Memory](#memory)
  * [Thread safety](#thread-safety)
* [6. Error Handling](#6-error-handling)

## 1. SkipMap

Module `xskiplist::map`, re-exported as `xskiplist::SkipMap`.

### SkipMap<K, V>

An ordered map. Keys are unique under `Ord`. Methods taking `&Q` require `K: Borrow<Q>` and `Q: Ord + ?Sized`.

* `fn new() -> Self`
* `fn len(&self) -> usize`
* `fn is_empty(&self) -> bool`
* `fn clear(&mut self)`: drops every element and frees the node memory the map holds alone.
* `fn first_key_value(&self) -> Option<(&K, &V)>`
* `fn last_key_value(&self) -> Option<(&K, &V)>`
* `fn pop_first(&mut self) -> Option<(K, V)>`
* `fn pop_last(&mut self) -> Option<(K, V)>`
* `fn get<Q>(&self, key: &Q) -> Option<&V>`
* `fn get_key_value<Q>(&self, key: &Q) -> Option<(&K, &V)>`
* `fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>`
* `fn contains_key<Q>(&self, key: &Q) -> bool`
* `fn insert(&mut self, key: K, value: V) -> Option<V> where K: Ord`: on a present key replaces the value, keeps the stored key and returns the old value.
* `fn remove<Q>(&mut self, key: &Q) -> Option<V>`
* `fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>`
* `fn entry(&mut self, key: K) -> Entry<'_, K, V> where K: Ord`
* `fn retain<F: FnMut(&K, &mut V) -> bool>(&mut self, f: F)`: visits in key order. If `f` panics the map stays consistent.
* `fn split_off<Q>(&mut self, key: &Q) -> Self where K: Borrow<Q> + Ord`: `self` keeps keys below `key`, the result gets the rest. O(log n).
* `fn append(&mut self, other: &mut Self) where K: Ord`: moves every element of `other` into `self` and leaves `other` empty. O(log n) when every key of `other` is greater than every key of `self`, otherwise O(n + m). A key present in both keeps the value from `other`.
* `fn iter(&self) -> Iter<'_, K, V>`
* `fn iter_mut(&mut self) -> IterMut<'_, K, V>`
* `fn keys(&self) -> Keys<'_, K, V>`
* `fn values(&self) -> Values<'_, K, V>`
* `fn values_mut(&mut self) -> ValuesMut<'_, K, V>`
* `fn into_keys(self) -> IntoKeys<K, V>`
* `fn into_values(self) -> IntoValues<K, V>`
* `fn range<T, R>(&self, range: R) -> Range<'_, K, V> where T: Ord + ?Sized, K: Borrow<T> + Ord, R: RangeBounds<T>`: panics on a start greater than the end and on equal excluded bounds.
* `fn range_mut<T, R>(&mut self, range: R) -> RangeMut<'_, K, V>`: same bounds and panics as `range`.

Trait implementations: `Default`, `Clone` (`K: Clone, V: Clone`), `FromIterator<(K, V)>` (`K: Ord`; a repeated key keeps its last value), `From<[(K, V); N]>`, `Extend<(K, V)>`, `Extend<(&K, &V)>` (`K: Copy, V: Copy`), `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug`, `Index<&Q>` (panics on a missing key), `IntoIterator` for `SkipMap`, `&SkipMap` and `&mut SkipMap`. `Send` when `K: Send, V: Send`. `Sync` when `K: Sync, V: Sync`.

### map::Entry<'a, K, V>

`enum Entry<'a, K, V> { Vacant(VacantEntry<'a, K, V>), Occupied(OccupiedEntry<'a, K, V>) }`

* `fn or_insert(self, default: V) -> &'a mut V`
* `fn or_insert_with<F: FnOnce() -> V>(self, default: F) -> &'a mut V`
* `fn or_insert_with_key<F: FnOnce(&K) -> V>(self, default: F) -> &'a mut V`
* `fn or_default(self) -> &'a mut V where V: Default`
* `fn key(&self) -> &K`
* `fn and_modify<F: FnOnce(&mut V)>(self, f: F) -> Self`

### map::VacantEntry<'a, K, V>

* `fn key(&self) -> &K`
* `fn into_key(self) -> K`
* `fn insert(self, value: V) -> &'a mut V`: inserts without a second search.

### map::OccupiedEntry<'a, K, V>

* `fn key(&self) -> &K`
* `fn get(&self) -> &V`
* `fn get_mut(&mut self) -> &mut V`
* `fn into_mut(self) -> &'a mut V`
* `fn insert(&mut self, value: V) -> V`: replaces the value and returns the old one.
* `fn remove(self) -> V where K: Ord`
* `fn remove_entry(self) -> (K, V) where K: Ord`

### Map iterators

All iterate in key order, implement `DoubleEndedIterator`, `ExactSizeIterator` and `FusedIterator`. They report the exact remaining length.

* `Iter<'a, K, V>`: `Item = (&'a K, &'a V)`. `Clone`.
* `IterMut<'a, K, V>`: `Item = (&'a K, &'a mut V)`.
* `IntoIter<K, V>`: `Item = (K, V)`. Dropping it drops the remaining elements.
* `Keys<'a, K, V>`, `Values<'a, K, V>`, `ValuesMut<'a, K, V>`, `IntoKeys<K, V>`, `IntoValues<K, V>`: projections of the above.
* `Range<'a, K, V>`: `Item = (&'a K, &'a V)`. `Clone`.
* `RangeMut<'a, K, V>`: `Item = (&'a K, &'a mut V)`.

## 2. SkipSet

Module `xskiplist::set`, re-exported as `xskiplist::SkipSet`.

### SkipSet<T>

An ordered set over `SkipMap<T, ()>`. Methods taking `&Q` require `T: Borrow<Q>` and `Q: Ord + ?Sized`.

* `fn new() -> Self`
* `fn len(&self) -> usize`
* `fn is_empty(&self) -> bool`
* `fn clear(&mut self)`
* `fn insert(&mut self, value: T) -> bool where T: Ord`: `false` when an equal value was present; the stored value is kept.
* `fn replace(&mut self, value: T) -> Option<T> where T: Ord`: stores `value`, returning the equal value it displaced.
* `fn contains<Q>(&self, value: &Q) -> bool`
* `fn get<Q>(&self, value: &Q) -> Option<&T>`
* `fn remove<Q>(&mut self, value: &Q) -> bool`
* `fn take<Q>(&mut self, value: &Q) -> Option<T>`
* `fn entry(&mut self, value: T) -> Entry<'_, T> where T: Ord`
* `fn first(&self) -> Option<&T>`
* `fn last(&self) -> Option<&T>`
* `fn pop_first(&mut self) -> Option<T>`
* `fn pop_last(&mut self) -> Option<T>`
* `fn retain<F: FnMut(&T) -> bool>(&mut self, f: F)`
* `fn split_off<Q>(&mut self, value: &Q) -> Self where T: Borrow<Q> + Ord`
* `fn append(&mut self, other: &mut Self) where T: Ord`: as `SkipMap::append`; a value present in both sides is kept once.
* `fn iter(&self) -> Iter<'_, T>`
* `fn range<Q, R>(&self, range: R) -> Range<'_, T> where Q: Ord + ?Sized, T: Borrow<Q> + Ord, R: RangeBounds<Q>`: same panics as `SkipMap::range`.
* `fn union<'a>(&'a self, other: &'a Self) -> Union<'a, T>`
* `fn intersection<'a>(&'a self, other: &'a Self) -> Intersection<'a, T>`
* `fn difference<'a>(&'a self, other: &'a Self) -> Difference<'a, T>`
* `fn symmetric_difference<'a>(&'a self, other: &'a Self) -> SymmetricDifference<'a, T>`

Trait implementations: `Default`, `Clone` (`T: Clone`), `FromIterator<T>` (`T: Ord`), `From<[T; N]>`, `Extend<T>`, `Extend<&T>` (`T: Copy`), `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug`, `IntoIterator` for `SkipSet` and `&SkipSet`. `BitOr`, `BitAnd`, `Sub` and `BitXor` on `&SkipSet<T>` with `T: Ord + Clone` return a new `SkipSet<T>` holding the union, intersection, difference and symmetric difference.

### set::Entry<'a, T>

`enum Entry<'a, T> { Vacant(VacantEntry<'a, T>), Occupied(OccupiedEntry<'a, T>) }`

### set::VacantEntry<'a, T>

* `fn get(&self) -> &T`
* `fn into_value(self) -> T`
* `fn insert(self) -> &'a T`

### set::OccupiedEntry<'a, T>

* `fn get(&self) -> &T`
* `fn remove(self) -> T where T: Ord`

### Set iterators

* `Iter<'a, T>`: `Item = &'a T`. `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.
* `Range<'a, T>`: `Item = &'a T`. `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.
* `IntoIter<T>`: `Item = T`. `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.
* `Union<'a, T>`, `Intersection<'a, T>`, `Difference<'a, T>`, `SymmetricDifference<'a, T>`: `Item = &'a T`, in order, `FusedIterator`, `T: Ord`. Forward only.

## 3. OrderedSkipList

Module `xskiplist::ordered`, re-exported as `xskiplist::OrderedSkipList`.

### OrderedSkipList<T>

A sorted list that keeps equal elements in insertion order and addresses elements by position. Positions are 0-based. Methods taking `&Q` require `T: Borrow<Q>` and `Q: Ord + ?Sized`.

* `fn new() -> Self`
* `fn len(&self) -> usize`
* `fn is_empty(&self) -> bool`
* `fn clear(&mut self)`
* `fn first(&self) -> Option<&T>`
* `fn last(&self) -> Option<&T>`
* `fn get_by_index(&self, index: usize) -> Option<&T>`
* `fn insert(&mut self, value: T) where T: Ord`: places `value` after every element equal to it.
* `fn contains<Q>(&self, value: &Q) -> bool`
* `fn get<Q>(&self, value: &Q) -> Option<&T>`: the first equal element.
* `fn rank<Q>(&self, value: &Q) -> Option<usize>`: the position of the first equal element.
* `fn count<Q>(&self, value: &Q) -> usize`: how many elements equal `value`.
* `fn take<Q>(&mut self, value: &Q) -> Option<T>`: removes the first equal element.
* `fn take_last<Q>(&mut self, value: &Q) -> Option<T>`: removes the last equal element.
* `fn remove(&mut self, index: usize) -> T`: panics when `index >= len`.
* `fn pop_first(&mut self) -> Option<T>`
* `fn pop_last(&mut self) -> Option<T>`
* `fn split_off<Q>(&mut self, value: &Q) -> Self`: `self` keeps elements below `value`.
* `fn split_off_index(&mut self, at: usize) -> Self`: `self` keeps the first `at` elements. Panics when `at > len`.
* `fn append(&mut self, other: &mut Self) where T: Ord`: O(log n) when every element of `other` is at least the last element of `self`, otherwise an O(n + m) merge in which equal elements of `self` stay ahead.
* `fn retain<F: FnMut(&T) -> bool>(&mut self, f: F)`
* `fn iter(&self) -> Iter<'_, T>`
* `fn range<Q, R>(&self, range: R) -> Range<'_, T> where Q: Ord + ?Sized, T: Borrow<Q>, R: RangeBounds<Q>`: same panics as `SkipMap::range`.

Trait implementations: `Default`, `Clone` (`T: Clone`), `FromIterator<T>` (`T: Ord`, a stable sort of the input), `From<[T; N]>`, `Extend<T>`, `Extend<&T>` (`T: Copy`), `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug` (as a list), `IntoIterator` for `OrderedSkipList` and `&OrderedSkipList`.

### Ordered list iterators

* `Iter<'a, T>`: `Item = &'a T`. `Clone`, `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.
* `Range<'a, T>`: same as `Iter`.
* `IntoIter<T>`: `Item = T`. `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.

## 4. SkipList

Module `xskiplist::list`, re-exported as `xskiplist::SkipList`.

### SkipList<T>

A list in insertion order with O(log n) access, insertion and removal at any index. Positions are 0-based.

* `fn new() -> Self`
* `fn len(&self) -> usize`
* `fn is_empty(&self) -> bool`
* `fn clear(&mut self)`
* `fn front(&self) -> Option<&T>`
* `fn front_mut(&mut self) -> Option<&mut T>`
* `fn back(&self) -> Option<&T>`
* `fn back_mut(&mut self) -> Option<&mut T>`
* `fn get(&self, index: usize) -> Option<&T>`
* `fn get_mut(&mut self, index: usize) -> Option<&mut T>`
* `fn push_front(&mut self, value: T)`
* `fn push_back(&mut self, value: T)`
* `fn pop_front(&mut self) -> Option<T>`
* `fn pop_back(&mut self) -> Option<T>`
* `fn insert(&mut self, index: usize, value: T)`: panics when `index > len`.
* `fn remove(&mut self, index: usize) -> T`: panics when `index >= len`.
* `fn split_off(&mut self, at: usize) -> Self`: `self` keeps the first `at` elements. Panics when `at > len`. O(log n).
* `fn append(&mut self, other: &mut Self)`: O(log n).
* `fn retain<F: FnMut(&T) -> bool>(&mut self, f: F)`
* `fn iter(&self) -> Iter<'_, T>`
* `fn iter_mut(&mut self) -> IterMut<'_, T>`

Trait implementations: `Default`, `Clone` (`T: Clone`), `FromIterator<T>` (input order kept), `From<[T; N]>`, `Extend<T>`, `Extend<&T>` (`T: Copy`), `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug` (as a list), `Index<usize>` and `IndexMut<usize>` (panic when out of range), `IntoIterator` for `SkipList`, `&SkipList` and `&mut SkipList`.

### List iterators

* `Iter<'a, T>`: `Item = &'a T`. `Clone`, `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.
* `IterMut<'a, T>`: `Item = &'a mut T`. `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.
* `IntoIter<T>`: `Item = T`. `DoubleEndedIterator`, `ExactSizeIterator`, `FusedIterator`.

## 5. Shared Behaviour

### Complexity

| Operation | Cost |
|---|---|
| lookup by key, insert, remove, `entry` | O(log n) expected |
| access by index, `rank`, `count` | O(log n) expected |
| `push_front`, `pop_first`, `pop_front` | O(1) expected |
| `push_back`, `pop_back`, `pop_last` | O(log n) expected |
| `split_off`, `split_off_index` | O(log n) expected |
| `append` with the second structure wholly after the first | O(log n) expected |
| `append` otherwise (map, set, ordered list) | O(n + m) |
| `range` construction | O(log n) expected |
| iteration, `retain`, `clear`, drop, `clone`, `FromIterator` from sorted input | O(n) |
| `FromIterator` from unsorted input (map, set, ordered list) | O(n log n) |

Tower heights are drawn with probability 1/4 per extra level and capped at 24 levels.

### Memory

A node holds its key, its value, a back pointer, its height and two words per tower level. Nodes are allocated from slabs owned by the structure, one slab per tower height, growing by doubling from 8 slots up to 1024 per chunk. Freed nodes are reused by the same structure. Structures related by `split_off` or `append` share one pool of chunks that is released when the last of them is dropped or cleared; `clear` releases the chunks only when no other structure shares them.

### Thread safety

Every structure is `Send` when its element types are `Send` and `Sync` when they are `Sync`. Borrowing iterators are `Send` and `Sync` when the element types are `Sync`. No structure is safe to mutate from two threads at once without external synchronisation.

## 6. Error Handling

No method returns `Result`. Conditions that cannot be satisfied are panics. Each is a caller error:

* `SkipList::insert(index, _)` and `SkipList::split_off(index)` with `index > len`.
* `SkipList::remove(index)`, `Index<usize>` and `IndexMut<usize>` on `SkipList` with `index >= len`.
* `OrderedSkipList::remove(index)` with `index >= len`; `OrderedSkipList::split_off_index(at)` with `at > len`.
* `Index<&Q>` on `SkipMap` with a key that is not present.
* `range` and `range_mut` on any structure with a start bound greater than the end bound.
* `range` and `range_mut` with both bounds `Excluded` and equal.
* Allocation failure, through `std::alloc::handle_alloc_error`.

Lookups return `Option`: `get`, `get_mut`, `get_key_value`, `get_by_index`, `first`, `last`, `pop_*`, `remove` and `take` on the map, set and ordered list, `rank`.
