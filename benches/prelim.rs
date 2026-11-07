//! Fixed-duration cells over every structure the `skiplist` crate ships, with its `hot` suite's
//! workloads plus the operations its per-type sweeps measure.
//!
//! Containers hold the even keys `0, 2, ..., 2 (n - 1)` and insertions use odd keys, so each lands
//! at a random position. `list/*` holds `0..n` and addresses by index. `osl/count_dups` holds
//! `i / 100` for `i` in `0..n`. Keys come from a fixed seeded pool. Each cell runs for `--limit`
//! seconds and reports steps and ns per step.
//!
//! `cargo bench --bench prelim -- [--limit S] [--impls new,original] [--sizes 1000,100000] [--only substr]`

use std::hint::black_box;
use std::time::{Duration, Instant};

const POOL: usize = 1 << 10;
const POINT: usize = 1024;
const BULK: usize = 1;
const DUPS: u64 = 100;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
}

fn pool(n: usize) -> Vec<u64> {
  let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
  (0..POOL).map(|_| rng.next() % n as u64).collect()
}

fn shuffled(n: usize) -> Vec<u64> {
  let mut keys: Vec<u64> = (0..n as u64).collect();
  let mut rng = Rng(2);
  for i in (1..keys.len()).rev() {
    keys.swap(i, (rng.next() % (i as u64 + 1)) as usize);
  }
  keys
}

// MARK: Structure APIs

trait ListApi: Clone {
  fn new() -> Self;
  fn get(&self, i: usize) -> Option<&u64>;
  fn insert(&mut self, i: usize, v: u64);
  fn remove(&mut self, i: usize) -> u64;
  fn push_front(&mut self, v: u64);
  fn push_back(&mut self, v: u64);
  fn pop_front(&mut self) -> Option<u64>;
  fn pop_back(&mut self) -> Option<u64>;
  fn sum(&self) -> u64;
  fn split_off(&mut self, at: usize) -> Self;
  fn append(&mut self, other: &mut Self);
  fn retain(&mut self, f: impl FnMut(&u64) -> bool);
  fn collect(it: impl Iterator<Item = u64>) -> Self;
}

trait OslApi: Clone {
  fn new() -> Self;
  fn insert(&mut self, v: u64);
  fn take(&mut self, v: &u64) -> Option<u64>;
  fn take_last(&mut self, v: &u64) -> Option<u64>;
  fn contains(&self, v: &u64) -> bool;
  fn rank(&self, v: &u64) -> Option<usize>;
  fn get_by_index(&self, i: usize) -> Option<&u64>;
  fn count(&self, v: &u64) -> usize;
  fn pop_first(&mut self) -> Option<u64>;
  fn sum(&self) -> u64;
  fn range_sum(&self, lo: u64, hi: u64) -> u64;
  fn split_off(&mut self, v: &u64) -> Self;
  fn append(&mut self, other: &mut Self);
  fn retain(&mut self, f: impl FnMut(&u64) -> bool);
  fn collect(it: impl Iterator<Item = u64>) -> Self;
}

trait SetApi<T: Ord>: Clone {
  fn new() -> Self;
  fn insert(&mut self, v: T) -> bool;
  fn remove(&mut self, v: &T) -> bool;
  fn replace(&mut self, v: T) -> Option<T>;
  fn entry_hit(&mut self, v: T) -> bool;
  fn contains(&self, v: &T) -> bool;
  fn range_next(&self, lo: T) -> Option<&T>;
  fn pop_first(&mut self) -> Option<T>;
  fn fold(&self, f: impl FnMut(u64, &T) -> u64) -> u64;
  fn into_fold(self, f: impl FnMut(u64, T) -> u64) -> u64;
  fn bitor(&self, other: &Self) -> Self;
  fn intersection_count(&self, other: &Self) -> usize;
  fn split_off(&mut self, v: &T) -> Self;
  fn append(&mut self, other: &mut Self);
  fn retain(&mut self, f: impl FnMut(&T) -> bool);
  fn collect(it: impl Iterator<Item = T>) -> Self;
}

trait MapApi: Clone {
  fn new() -> Self;
  fn insert(&mut self, k: u64, v: u64) -> Option<u64>;
  fn remove(&mut self, k: &u64) -> Option<u64>;
  fn get(&self, k: &u64) -> Option<&u64>;
  fn get_mut(&mut self, k: &u64) -> Option<&mut u64>;
  fn entry_or_insert(&mut self, k: u64, v: u64) -> &mut u64;
  fn pop_last(&mut self) -> Option<(u64, u64)>;
  fn fold(&self) -> u64;
  fn split_off(&mut self, k: &u64) -> Self;
  fn append(&mut self, other: &mut Self);
  fn retain(&mut self, f: impl FnMut(&u64, &mut u64) -> bool);
  fn collect(it: impl Iterator<Item = (u64, u64)>) -> Self;
}

// MARK: new

impl ListApi for xskiplist::SkipList<u64> {
  fn new() -> Self {
    Self::new()
  }
  fn get(&self, i: usize) -> Option<&u64> {
    self.get(i)
  }
  fn insert(&mut self, i: usize, v: u64) {
    self.insert(i, v)
  }
  fn remove(&mut self, i: usize) -> u64 {
    self.remove(i)
  }
  fn push_front(&mut self, v: u64) {
    self.push_front(v)
  }
  fn push_back(&mut self, v: u64) {
    self.push_back(v)
  }
  fn pop_front(&mut self) -> Option<u64> {
    self.pop_front()
  }
  fn pop_back(&mut self) -> Option<u64> {
    self.pop_back()
  }
  fn sum(&self) -> u64 {
    self.iter().fold(0, |a, v| a.wrapping_add(*v))
  }
  fn split_off(&mut self, at: usize) -> Self {
    self.split_off(at)
  }
  fn append(&mut self, other: &mut Self) {
    self.append(other)
  }
  fn retain(&mut self, f: impl FnMut(&u64) -> bool) {
    self.retain(f)
  }
  fn collect(it: impl Iterator<Item = u64>) -> Self {
    it.collect()
  }
}

impl OslApi for xskiplist::OrderedSkipList<u64> {
  fn new() -> Self {
    Self::new()
  }
  fn insert(&mut self, v: u64) {
    self.insert(v)
  }
  fn take(&mut self, v: &u64) -> Option<u64> {
    self.take(v)
  }
  fn take_last(&mut self, v: &u64) -> Option<u64> {
    self.take_last(v)
  }
  fn contains(&self, v: &u64) -> bool {
    self.contains(v)
  }
  fn rank(&self, v: &u64) -> Option<usize> {
    self.rank(v)
  }
  fn get_by_index(&self, i: usize) -> Option<&u64> {
    self.get_by_index(i)
  }
  fn count(&self, v: &u64) -> usize {
    self.count(v)
  }
  fn pop_first(&mut self) -> Option<u64> {
    self.pop_first()
  }
  fn sum(&self) -> u64 {
    self.iter().fold(0, |a, v| a.wrapping_add(*v))
  }
  fn range_sum(&self, lo: u64, hi: u64) -> u64 {
    self.range(lo..hi).fold(0, |a, v| a.wrapping_add(*v))
  }
  fn split_off(&mut self, v: &u64) -> Self {
    self.split_off(v)
  }
  fn append(&mut self, other: &mut Self) {
    self.append(other)
  }
  fn retain(&mut self, f: impl FnMut(&u64) -> bool) {
    self.retain(f)
  }
  fn collect(it: impl Iterator<Item = u64>) -> Self {
    it.collect()
  }
}

impl<T: Ord + Clone> SetApi<T> for xskiplist::SkipSet<T> {
  fn new() -> Self {
    Self::new()
  }
  fn insert(&mut self, v: T) -> bool {
    self.insert(v)
  }
  fn remove(&mut self, v: &T) -> bool {
    self.remove(v)
  }
  fn replace(&mut self, v: T) -> Option<T> {
    self.replace(v)
  }
  fn entry_hit(&mut self, v: T) -> bool {
    match self.entry(v) {
      xskiplist::set::Entry::Occupied(e) => {
        black_box(e.get());
        true
      }
      xskiplist::set::Entry::Vacant(_) => false,
    }
  }
  fn contains(&self, v: &T) -> bool {
    self.contains(v)
  }
  fn range_next(&self, lo: T) -> Option<&T> {
    self.range(lo..).next()
  }
  fn pop_first(&mut self) -> Option<T> {
    self.pop_first()
  }
  fn fold(&self, f: impl FnMut(u64, &T) -> u64) -> u64 {
    self.iter().fold(0, f)
  }
  fn into_fold(self, f: impl FnMut(u64, T) -> u64) -> u64 {
    self.into_iter().fold(0, f)
  }
  fn bitor(&self, other: &Self) -> Self {
    self | other
  }
  fn intersection_count(&self, other: &Self) -> usize {
    self.intersection(other).count()
  }
  fn split_off(&mut self, v: &T) -> Self {
    self.split_off(v)
  }
  fn append(&mut self, other: &mut Self) {
    self.append(other)
  }
  fn retain(&mut self, f: impl FnMut(&T) -> bool) {
    self.retain(f)
  }
  fn collect(it: impl Iterator<Item = T>) -> Self {
    it.collect()
  }
}

impl MapApi for xskiplist::SkipMap<u64, u64> {
  fn new() -> Self {
    Self::new()
  }
  fn insert(&mut self, k: u64, v: u64) -> Option<u64> {
    self.insert(k, v)
  }
  fn remove(&mut self, k: &u64) -> Option<u64> {
    self.remove(k)
  }
  fn get(&self, k: &u64) -> Option<&u64> {
    self.get(k)
  }
  fn get_mut(&mut self, k: &u64) -> Option<&mut u64> {
    self.get_mut(k)
  }
  fn entry_or_insert(&mut self, k: u64, v: u64) -> &mut u64 {
    self.entry(k).or_insert(v)
  }
  fn pop_last(&mut self) -> Option<(u64, u64)> {
    self.pop_last()
  }
  fn fold(&self) -> u64 {
    self.iter().fold(0, |a, (k, v)| a.wrapping_add(*k ^ *v))
  }
  fn split_off(&mut self, k: &u64) -> Self {
    self.split_off(k)
  }
  fn append(&mut self, other: &mut Self) {
    self.append(other)
  }
  fn retain(&mut self, f: impl FnMut(&u64, &mut u64) -> bool) {
    self.retain(f)
  }
  fn collect(it: impl Iterator<Item = (u64, u64)>) -> Self {
    it.collect()
  }
}

// MARK: original

fn generator() -> skiplist::Geometric {
  skiplist::Geometric::new_with_seed(16, 0.5, 42).expect("valid parameters")
}

#[derive(Clone)]
struct OList(skiplist::SkipList<u64>);
#[derive(Clone)]
struct OOsl(skiplist::OrderedSkipList<u64>);
#[derive(Clone)]
struct OSet<T: Ord>(skiplist::SkipSet<T>);
#[derive(Clone)]
struct OMap(skiplist::SkipMap<u64, u64>);

impl ListApi for OList {
  fn new() -> Self {
    OList(skiplist::SkipList::with_level_generator(generator()))
  }
  fn get(&self, i: usize) -> Option<&u64> {
    self.0.get(i)
  }
  fn insert(&mut self, i: usize, v: u64) {
    self.0.insert(i, v)
  }
  fn remove(&mut self, i: usize) -> u64 {
    self.0.remove(i)
  }
  fn push_front(&mut self, v: u64) {
    self.0.push_front(v)
  }
  fn push_back(&mut self, v: u64) {
    self.0.push_back(v)
  }
  fn pop_front(&mut self) -> Option<u64> {
    self.0.pop_front()
  }
  fn pop_back(&mut self) -> Option<u64> {
    self.0.pop_back()
  }
  fn sum(&self) -> u64 {
    self.0.iter().fold(0, |a, v| a.wrapping_add(*v))
  }
  fn split_off(&mut self, at: usize) -> Self {
    OList(self.0.split_off(at))
  }
  fn append(&mut self, other: &mut Self) {
    self.0.append(&mut other.0)
  }
  fn retain(&mut self, f: impl FnMut(&u64) -> bool) {
    self.0.retain(f)
  }
  fn collect(it: impl Iterator<Item = u64>) -> Self {
    OList(it.collect())
  }
}

impl OslApi for OOsl {
  fn new() -> Self {
    OOsl(skiplist::OrderedSkipList::with_level_generator(generator()))
  }
  fn insert(&mut self, v: u64) {
    self.0.insert(v)
  }
  fn take(&mut self, v: &u64) -> Option<u64> {
    self.0.take(v)
  }
  fn take_last(&mut self, v: &u64) -> Option<u64> {
    self.0.take_last(v)
  }
  fn contains(&self, v: &u64) -> bool {
    self.0.contains(v)
  }
  fn rank(&self, v: &u64) -> Option<usize> {
    self.0.rank(v)
  }
  fn get_by_index(&self, i: usize) -> Option<&u64> {
    self.0.get_by_index(i)
  }
  fn count(&self, v: &u64) -> usize {
    self.0.count(v)
  }
  fn pop_first(&mut self) -> Option<u64> {
    self.0.pop_first()
  }
  fn sum(&self) -> u64 {
    self.0.iter().fold(0, |a, v| a.wrapping_add(*v))
  }
  fn range_sum(&self, lo: u64, hi: u64) -> u64 {
    self.0.range(lo..hi).fold(0, |a, v| a.wrapping_add(*v))
  }
  fn split_off(&mut self, v: &u64) -> Self {
    OOsl(self.0.split_off(v))
  }
  fn append(&mut self, other: &mut Self) {
    self.0.append(&mut other.0)
  }
  fn retain(&mut self, f: impl FnMut(&u64) -> bool) {
    self.0.retain(f)
  }
  fn collect(it: impl Iterator<Item = u64>) -> Self {
    OOsl(it.collect())
  }
}

impl<T: Ord + Clone> SetApi<T> for OSet<T> {
  fn new() -> Self {
    OSet(skiplist::SkipSet::with_level_generator(generator()))
  }
  fn insert(&mut self, v: T) -> bool {
    self.0.insert(v)
  }
  fn remove(&mut self, v: &T) -> bool {
    self.0.remove(v)
  }
  fn replace(&mut self, v: T) -> Option<T> {
    self.0.replace(v)
  }
  fn entry_hit(&mut self, v: T) -> bool {
    match self.0.entry(v) {
      skiplist::skip_set::Entry::Occupied(e) => {
        black_box(e.get());
        true
      }
      skiplist::skip_set::Entry::Vacant(_) => false,
    }
  }
  fn contains(&self, v: &T) -> bool {
    self.0.contains(v)
  }
  fn range_next(&self, lo: T) -> Option<&T> {
    self.0.range(lo..).next()
  }
  fn pop_first(&mut self) -> Option<T> {
    self.0.pop_first()
  }
  fn fold(&self, f: impl FnMut(u64, &T) -> u64) -> u64 {
    self.0.iter().fold(0, f)
  }
  fn into_fold(self, f: impl FnMut(u64, T) -> u64) -> u64 {
    self.0.into_iter().fold(0, f)
  }
  fn bitor(&self, other: &Self) -> Self {
    OSet(&self.0 | &other.0)
  }
  fn intersection_count(&self, other: &Self) -> usize {
    self.0.intersection(&other.0).count()
  }
  fn split_off(&mut self, v: &T) -> Self {
    OSet(self.0.split_off(v))
  }
  fn append(&mut self, other: &mut Self) {
    self.0.append(&mut other.0)
  }
  fn retain(&mut self, f: impl FnMut(&T) -> bool) {
    self.0.retain(f)
  }
  fn collect(it: impl Iterator<Item = T>) -> Self {
    OSet(it.collect())
  }
}

impl MapApi for OMap {
  fn new() -> Self {
    OMap(skiplist::SkipMap::with_level_generator(generator()))
  }
  fn insert(&mut self, k: u64, v: u64) -> Option<u64> {
    self.0.insert(k, v)
  }
  fn remove(&mut self, k: &u64) -> Option<u64> {
    self.0.remove(k)
  }
  fn get(&self, k: &u64) -> Option<&u64> {
    self.0.get(k)
  }
  fn get_mut(&mut self, k: &u64) -> Option<&mut u64> {
    self.0.get_mut(k)
  }
  fn entry_or_insert(&mut self, k: u64, v: u64) -> &mut u64 {
    self.0.entry(k).or_insert(v)
  }
  fn pop_last(&mut self) -> Option<(u64, u64)> {
    self.0.pop_last()
  }
  fn fold(&self) -> u64 {
    self.0.iter().fold(0, |a, (k, v)| a.wrapping_add(*k ^ *v))
  }
  fn split_off(&mut self, k: &u64) -> Self {
    OMap(self.0.split_off(k))
  }
  fn append(&mut self, other: &mut Self) {
    self.0.append(&mut other.0)
  }
  fn retain(&mut self, f: impl FnMut(&u64, &mut u64) -> bool) {
    self.0.retain(f)
  }
  fn collect(it: impl Iterator<Item = (u64, u64)>) -> Self {
    OMap(it.collect())
  }
}

// MARK: Cells

struct Cell {
  label: &'static str,
  run: Box<dyn Fn(usize, Duration) -> (u64, Duration)>,
}

fn cell<S: 'static>(
  label: &'static str,
  batch: usize,
  setup: impl Fn(usize) -> S + 'static,
  step: impl Fn(&mut S, u64) + 'static,
) -> Cell {
  Cell {
    label,
    run: Box::new(move |n, limit| {
      let keys = pool(n);
      let mut state = setup(n);
      let start = Instant::now();
      let mut steps = 0u64;
      let mut timed = Duration::ZERO;
      let mut i = 0usize;
      while start.elapsed() < limit {
        let t = Instant::now();
        for _ in 0..batch {
          step(&mut state, black_box(keys[i & (POOL - 1)]));
          i += 1;
        }
        timed += t.elapsed();
        steps += batch as u64;
      }
      black_box(&mut state);
      (steps, timed)
    }),
  }
}

fn list<L: ListApi>(n: usize) -> L {
  let mut l = L::new();
  for i in 0..n as u64 {
    l.push_back(i);
  }
  l
}

fn osl<O: OslApi>(n: usize) -> O {
  let mut o = O::new();
  for i in 0..n as u64 {
    o.insert(2 * i);
  }
  o
}

fn set<S: SetApi<u64>>(n: usize) -> S {
  let mut s = S::new();
  for i in 0..n as u64 {
    s.insert(2 * i);
  }
  s
}

fn map<M: MapApi>(n: usize) -> M {
  let mut m = M::new();
  for i in 0..n as u64 {
    m.insert(2 * i, i);
  }
  m
}

fn key_string(k: u64) -> String {
  format!("key-{k:08}")
}

fn cells<L, O, S, SS, M>() -> Vec<Cell>
where
  L: ListApi + 'static,
  O: OslApi + 'static,
  S: SetApi<u64> + 'static,
  SS: SetApi<String> + 'static,
  M: MapApi + 'static,
{
  vec![
    cell("list/get", POINT, list::<L>, |l, k| {
      black_box(l.get(k as usize));
    }),
    cell("list/insert_remove", POINT, list::<L>, |l, k| {
      l.insert(k as usize, k);
      black_box(l.remove(k as usize));
    }),
    cell("list/queue", POINT, list::<L>, |l, k| {
      l.push_back(k);
      black_box(l.pop_front());
    }),
    cell("list/stack", POINT, list::<L>, |l, k| {
      l.push_back(k);
      black_box(l.pop_back());
    }),
    cell("list/push_front_pop_back", POINT, list::<L>, |l, k| {
      l.push_front(k);
      black_box(l.pop_back());
    }),
    cell("list/iter", BULK, list::<L>, |l, _| {
      black_box(l.sum());
    }),
    cell("list/clone", BULK, list::<L>, |l, _| {
      black_box(l.clone());
    }),
    cell("list/split_append", BULK, list::<L>, |l, k| {
      let mut tail = l.split_off(k as usize);
      l.append(black_box(&mut tail));
    }),
    cell(
      "list/from_iter",
      BULK,
      |n| n,
      |n, _| {
        black_box(L::collect(0..*n as u64));
      },
    ),
    cell(
      "list/retain",
      BULK,
      |n| n,
      |n, _| {
        let mut l = L::collect(0..*n as u64);
        l.retain(|v| v % 2 == 0);
        black_box(l);
      },
    ),
    cell("osl/insert_take", POINT, osl::<O>, |o, k| {
      let v = 2 * k + 1;
      o.insert(v);
      black_box(o.take(&v));
    }),
    cell("osl/insert_take_last", POINT, osl::<O>, |o, k| {
      let v = 2 * k + 1;
      o.insert(v);
      black_box(o.take_last(&v));
    }),
    cell("osl/contains", POINT, osl::<O>, |o, k| {
      black_box(o.contains(&(2 * k)));
    }),
    cell("osl/rank", POINT, osl::<O>, |o, k| {
      black_box(o.rank(&(2 * k)));
    }),
    cell("osl/get_by_index", POINT, osl::<O>, |o, k| {
      black_box(o.get_by_index(k as usize));
    }),
    cell(
      "osl/count_dups",
      POINT,
      |n| {
        let mut o = O::new();
        for i in 0..n as u64 {
          o.insert(i / DUPS);
        }
        o
      },
      |o, k| {
        black_box(o.count(&(k / DUPS)));
      },
    ),
    cell("osl/pop_first_insert", POINT, osl::<O>, |o, _| {
      let v = o.pop_first().expect("non-empty");
      o.insert(black_box(v));
    }),
    cell("osl/iter", BULK, osl::<O>, |o, _| {
      black_box(o.sum());
    }),
    cell("osl/range_iter", POINT, osl::<O>, |o, k| {
      black_box(o.range_sum(2 * k, 2 * k + 200));
    }),
    cell("osl/split_append", BULK, osl::<O>, |o, k| {
      let mut tail = o.split_off(&(2 * k + 1));
      o.append(black_box(&mut tail));
    }),
    cell("osl/from_iter", BULK, shuffled, |keys, _| {
      black_box(O::collect(keys.iter().copied()));
    }),
    cell(
      "osl/retain",
      BULK,
      |n| n,
      |n, _| {
        let mut o = O::collect(0..*n as u64);
        o.retain(|v| v % 2 == 0);
        black_box(o);
      },
    ),
    cell("set/insert_remove", POINT, set::<S>, |s, k| {
      let v = 2 * k + 1;
      black_box(s.insert(v));
      black_box(s.remove(&v));
    }),
    cell("set/insert_present", POINT, set::<S>, |s, k| {
      black_box(s.insert(2 * k));
    }),
    cell("set/replace", POINT, set::<S>, |s, k| {
      black_box(s.replace(2 * k));
    }),
    cell("set/entry_hit", POINT, set::<S>, |s, k| {
      black_box(s.entry_hit(2 * k));
    }),
    cell("set/contains", POINT, set::<S>, |s, k| {
      black_box(s.contains(&(2 * k)));
    }),
    cell(
      "set/contains_string",
      POINT,
      |n| {
        let queries: Vec<String> = (0..n as u64).map(|i| key_string(2 * i)).collect();
        let s = SS::collect(queries.iter().cloned());
        (s, queries)
      },
      |(s, queries), k| {
        black_box(s.contains(&queries[k as usize]));
      },
    ),
    cell(
      "set/insert_remove_string",
      POINT,
      |n| {
        let s = SS::collect((0..n as u64).map(|i| key_string(2 * i)));
        let absent: Vec<String> = (0..n as u64).map(|i| key_string(2 * i + 1)).collect();
        (s, absent)
      },
      |(s, absent), k| {
        let v = &absent[k as usize];
        black_box(s.insert(v.clone()));
        black_box(s.remove(v));
      },
    ),
    cell("set/range_next", POINT, set::<S>, |s, k| {
      black_box(s.range_next(2 * k + 1));
    }),
    cell("set/pop_first_insert", POINT, set::<S>, |s, _| {
      let v = s.pop_first().expect("non-empty");
      s.insert(black_box(v));
    }),
    cell("set/iter", BULK, set::<S>, |s, _| {
      black_box(s.fold(|a, v| a.wrapping_add(*v)));
    }),
    cell(
      "set/from_iter",
      BULK,
      |n| n,
      |n, _| {
        black_box(S::collect(0..*n as u64));
      },
    ),
    cell("set/from_iter_shuffled", BULK, shuffled, |keys, _| {
      black_box(S::collect(keys.iter().copied()));
    }),
    cell("set/clone", BULK, set::<S>, |s, _| {
      black_box(s.clone());
    }),
    cell("set/into_iter", BULK, set::<S>, |s, _| {
      black_box(s.clone().into_fold(|a, v| a.wrapping_add(v)));
    }),
    cell(
      "set/bitor",
      BULK,
      |n| {
        let a = S::collect((0..n as u64).map(|i| 2 * i).filter(|v| v % 4 == 0));
        let b = S::collect((0..n as u64).map(|i| 2 * i).filter(|v| v % 4 != 0));
        (a, b)
      },
      |(a, b), _| {
        black_box(a.bitor(b));
      },
    ),
    cell(
      "set/intersection",
      BULK,
      |n| {
        let a = S::collect((0..n as u64).filter(|v| v % 3 == 0));
        let b = S::collect((0..n as u64).filter(|v| v % 2 == 0));
        (a, b)
      },
      |(a, b), _| {
        black_box(a.intersection_count(b));
      },
    ),
    cell("set/split_append", BULK, set::<S>, |s, k| {
      let mut tail = s.split_off(&(2 * k + 1));
      s.append(black_box(&mut tail));
    }),
    cell(
      "set/retain",
      BULK,
      |n| n,
      |n, _| {
        let mut s = S::collect(0..*n as u64);
        s.retain(|v| v % 2 == 0);
        black_box(s);
      },
    ),
    cell("map/insert_remove", POINT, map::<M>, |m, k| {
      let key = 2 * k + 1;
      black_box(m.insert(key, k));
      black_box(m.remove(&key));
    }),
    cell("map/insert_replace", POINT, map::<M>, |m, k| {
      black_box(m.insert(2 * k, k));
    }),
    cell("map/get", POINT, map::<M>, |m, k| {
      black_box(m.get(&(2 * k)));
    }),
    cell("map/get_mut", POINT, map::<M>, |m, k| {
      if let Some(v) = m.get_mut(&(2 * k)) {
        *v = v.wrapping_add(1);
      }
    }),
    cell("map/entry_hit", POINT, map::<M>, |m, k| {
      let v = m.entry_or_insert(2 * k, 0);
      *v = v.wrapping_add(1);
    }),
    cell("map/pop_last", POINT, map::<M>, |m, _| {
      let (k, v) = m.pop_last().expect("non-empty");
      m.insert(black_box(k), v);
    }),
    cell("map/iter", BULK, map::<M>, |m, _| {
      black_box(m.fold());
    }),
    cell("map/split_append", BULK, map::<M>, |m, k| {
      let mut tail = m.split_off(&(2 * k + 1));
      m.append(black_box(&mut tail));
    }),
    cell("map/from_iter", BULK, shuffled, |keys, _| {
      black_box(M::collect(keys.iter().map(|k| (*k, *k))));
    }),
    cell(
      "map/retain",
      BULK,
      |n| n,
      |n, _| {
        let mut m = M::collect((0..*n as u64).map(|k| (k, k)));
        m.retain(|k, _| k % 2 == 0);
        black_box(m);
      },
    ),
  ]
}

fn list_arg(args: &[String], flag: &str, default: &[&str]) -> Vec<String> {
  match args.iter().position(|a| a == flag) {
    Some(i) => args[i + 1].split(',').map(str::to_owned).collect(),
    None => default.iter().map(|s| s.to_string()).collect(),
  }
}

fn main() {
  let args: Vec<String> = std::env::args().collect();
  let limit = Duration::from_secs_f64(list_arg(&args, "--limit", &["10"])[0].parse().expect("--limit seconds"));
  let impls = list_arg(&args, "--impls", &["new", "original"]);
  let sizes: Vec<usize> = list_arg(&args, "--sizes", &["1000", "100000"])
    .iter()
    .map(|s| s.parse().expect("size"))
    .collect();
  let only = args.iter().position(|a| a == "--only").map(|i| args[i + 1].clone());
  let new = cells::<
    xskiplist::SkipList<u64>,
    xskiplist::OrderedSkipList<u64>,
    xskiplist::SkipSet<u64>,
    xskiplist::SkipSet<String>,
    xskiplist::SkipMap<u64, u64>,
  >();
  let original = cells::<OList, OOsl, OSet<u64>, OSet<String>, OMap>();
  println!(
    "{:<9} {:<26} {:>7} {:>12} {:>12}",
    "impl", "workload", "n", "steps", "ns/step"
  );
  for (i, label) in new.iter().map(|c| c.label).enumerate() {
    if only.as_deref().is_some_and(|o| !label.contains(o)) {
      continue;
    }
    for &n in &sizes {
      for name in &impls {
        let cell = match name.as_str() {
          "new" => &new[i],
          "original" => &original[i],
          other => panic!("unknown impl {other}"),
        };
        let (steps, timed) = (cell.run)(n, limit);
        let ns = timed.as_nanos() as f64 / steps.max(1) as f64;
        println!("{:<9} {:<26} {:>7} {:>12} {:>12.1}", name, label, n, steps, ns);
      }
    }
  }
}
