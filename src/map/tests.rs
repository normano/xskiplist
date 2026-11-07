use super::SkipMap;
use crate::test_util::scaled;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ops::Bound;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }

  fn below(&mut self, n: u64) -> u64 {
    self.next() % n
  }
}

fn assert_same<K, V>(map: &SkipMap<K, V>, oracle: &BTreeMap<K, V>)
where
  K: Ord + std::fmt::Debug,
  V: PartialEq + std::fmt::Debug,
{
  map.check_invariants();
  assert_eq!(map.len(), oracle.len());
  assert!(map.iter().eq(oracle.iter()));
  assert!(map.iter().rev().eq(oracle.iter().rev()));
  assert_eq!(map.first_key_value(), oracle.first_key_value());
  assert_eq!(map.last_key_value(), oracle.last_key_value());
}

#[test]
fn random_operations_match_btreemap() {
  for seed in 1..=8u64 {
    for key_space in [4u64, 64, 4096] {
      let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
      let mut map = SkipMap::new();
      let mut oracle = BTreeMap::new();
      for step in 0..scaled(4000) {
        let key = rng.below(key_space);
        let value = rng.next();
        match rng.below(10) {
          0..=3 => assert_eq!(map.insert(key, value), oracle.insert(key, value)),
          4 | 5 => assert_eq!(map.remove(&key), oracle.remove(&key)),
          6 => assert_eq!(map.get(&key), oracle.get(&key)),
          7 => {
            if let (Some(a), Some(b)) = (map.get_mut(&key), oracle.get_mut(&key)) {
              *a = value;
              *b = value;
            }
            assert_eq!(map.contains_key(&key), oracle.contains_key(&key));
          }
          8 => assert_eq!(map.pop_first(), oracle.pop_first()),
          _ => assert_eq!(map.pop_last(), oracle.pop_last()),
        }
        if step % 97 == 0 {
          assert_same(&map, &oracle);
        }
      }
      assert_same(&map, &oracle);
    }
  }
}

#[test]
fn ranges_match_btreemap() {
  let mut rng = Rng(77);
  let mut map = SkipMap::new();
  let mut oracle = BTreeMap::new();
  for _ in 0..300 {
    let k = rng.below(200) * 2;
    map.insert(k, k);
    oracle.insert(k, k);
  }
  let bound = |rng: &mut Rng| match rng.below(3) {
    0 => Bound::Unbounded,
    1 => Bound::Included(rng.below(410)),
    _ => Bound::Excluded(rng.below(410)),
  };
  for _ in 0..scaled(3000) {
    let range = (bound(&mut rng), bound(&mut rng));
    let expected = catch_unwind(|| oracle.range(range).map(|(k, v)| (*k, *v)).collect::<Vec<_>>());
    let actual = catch_unwind(AssertUnwindSafe(|| {
      map.range(range).map(|(k, v)| (*k, *v)).collect::<Vec<_>>()
    }));
    match (expected, actual) {
      (Ok(e), Ok(a)) => {
        assert_eq!(a, e, "{range:?}");
        let back: Vec<_> = map.range(range).rev().map(|(k, v)| (*k, *v)).collect();
        assert!(back.iter().rev().eq(e.iter()), "{range:?}");
      }
      (Err(_), Err(_)) => {}
      (e, a) => panic!("{range:?}: oracle panicked {} map panicked {}", e.is_err(), a.is_err()),
    }
  }
}

#[test]
fn range_meets_in_the_middle() {
  let map: SkipMap<u32, u32> = (0..10).map(|k| (k, k)).collect();
  let mut range = map.range(2..8);
  let mut seen = Vec::new();
  loop {
    match (range.next(), range.next_back()) {
      (Some(a), Some(b)) => {
        seen.push(*a.0);
        seen.push(*b.0);
      }
      (Some(a), None) => seen.push(*a.0),
      (None, _) => break,
    }
  }
  seen.sort();
  assert_eq!(seen, vec![2, 3, 4, 5, 6, 7]);
}

#[test]
fn range_mut_updates_values() {
  let mut map: SkipMap<u32, u32> = (0..20).map(|k| (k, 0)).collect();
  for (_, v) in map.range_mut(5..=9) {
    *v = 1;
  }
  let ones: Vec<u32> = map.iter().filter(|(_, v)| **v == 1).map(|(k, _)| *k).collect();
  assert_eq!(ones, vec![5, 6, 7, 8, 9]);
  map.check_invariants();
}

#[test]
fn borrowed_lookups() {
  let mut map = SkipMap::new();
  map.insert(String::from("b"), 2);
  map.insert(String::from("a"), 1);
  assert_eq!(map.get("a"), Some(&1));
  assert_eq!(map["b"], 2);
  assert_eq!(
    map
      .range::<str, _>((Bound::Included("a"), Bound::Excluded("b")))
      .count(),
    1
  );
  assert_eq!(map.remove("a"), Some(1));
  assert!(!map.contains_key("a"));
}

#[test]
#[should_panic]
fn index_of_missing_key_panics() {
  let map: SkipMap<u32, u32> = SkipMap::new();
  let _ = map[&3];
}

#[test]
fn iterators_and_views() {
  let mut map: SkipMap<u32, u32> = (0..50).rev().map(|k| (k, k * 10)).collect();
  assert_eq!(map.iter().len(), 50);
  assert!(map.keys().copied().eq(0..50));
  assert!(map.values().copied().eq((0..50).map(|k| k * 10)));
  for v in map.values_mut() {
    *v += 1;
  }
  for (k, v) in map.iter_mut() {
    assert_eq!(*v, *k * 10 + 1);
  }
  let mut it = map.iter();
  assert_eq!(it.next(), Some((&0, &1)));
  assert_eq!(it.next_back(), Some((&49, &491)));
  assert_eq!(it.len(), 48);
  let owned: Vec<(u32, u32)> = map.clone().into_iter().collect();
  assert_eq!(owned.len(), 50);
  assert!(map.clone().into_keys().eq(0..50));
  assert!(map.clone().into_values().rev().eq((0..50).rev().map(|k| k * 10 + 1)));
  let by_ref: Vec<_> = (&map).into_iter().map(|(k, _)| *k).collect();
  assert_eq!(by_ref.len(), 50);
}

#[test]
fn from_iter_keeps_last_duplicate_like_btreemap() {
  let pairs = [(3, "a"), (1, "b"), (3, "c"), (2, "d"), (1, "e")];
  let map: SkipMap<_, _> = pairs.iter().copied().collect();
  let oracle: BTreeMap<_, _> = pairs.iter().copied().collect();
  assert_same(&map, &oracle);
  let mut map = map;
  let mut oracle = oracle;
  map.extend([(5, "x"), (3, "y")]);
  oracle.extend([(5, "x"), (3, "y")]);
  assert_same(&map, &oracle);
}

#[test]
fn retain_matches_btreemap() {
  let mut map: SkipMap<u32, u32> = (0..1000).map(|k| (k, k)).collect();
  let mut oracle: BTreeMap<u32, u32> = (0..1000).map(|k| (k, k)).collect();
  map.retain(|k, v| {
    *v += 1;
    k % 3 != 0
  });
  oracle.retain(|k, v| {
    *v += 1;
    k % 3 != 0
  });
  assert_same(&map, &oracle);
  map.retain(|_, _| false);
  assert!(map.is_empty());
  map.check_invariants();
  map.insert(1, 1);
  map.check_invariants();
}

#[test]
fn retain_panic_leaves_map_consistent() {
  let mut map: SkipMap<u32, u32> = (0..100).map(|k| (k, k)).collect();
  let result = catch_unwind(AssertUnwindSafe(|| {
    map.retain(|k, _| {
      if *k == 60 {
        panic!("stop");
      }
      k % 2 == 0
    })
  }));
  assert!(result.is_err());
  map.check_invariants();
  let expected: Vec<u32> = (0..60).filter(|k| k % 2 == 0).chain(60..100).collect();
  assert!(map.keys().copied().eq(expected));
  for k in 0..200 {
    map.insert(k, k);
  }
  map.check_invariants();
  assert_eq!(map.len(), 200);
}

#[derive(Clone)]
struct Counted(Rc<Cell<usize>>);

impl Drop for Counted {
  fn drop(&mut self) {
    self.0.set(self.0.get() + 1);
  }
}

#[test]
fn every_key_and_value_dropped_once() {
  let drops = Rc::new(Cell::new(0));
  let c = || Counted(drops.clone());
  {
    let mut map = SkipMap::new();
    for k in 0..100u32 {
      map.insert(k, (c(), c()));
    }
    assert_eq!(drops.get(), 0);
    drop(map.insert(5, (c(), c())));
    assert_eq!(drops.get(), 2);
    drop(map.remove(&6));
    assert_eq!(drops.get(), 4);
    drop(map.pop_first());
    drop(map.pop_last());
    assert_eq!(drops.get(), 8);
    map.retain(|k, _| *k >= 50);
    assert_eq!(drops.get(), 8 + 2 * 48);
    let clone = map.clone();
    let mut it = clone.into_iter();
    drop(it.next());
    drop(it.next_back());
    drop(it);
    map.clear();
    assert!(map.is_empty());
    for k in 0..10u32 {
      map.insert(k, (c(), c()));
    }
  }
  assert_eq!(drops.get(), 2 * (100 + 1 + 49 + 10));
}

#[test]
fn clone_eq_ord_hash_debug_follow_contents() {
  use std::collections::hash_map::DefaultHasher;
  use std::hash::{Hash, Hasher};
  let a: SkipMap<u32, u32> = (0..10).map(|k| (k, k)).collect();
  let b = a.clone();
  b.check_invariants();
  assert_eq!(a, b);
  let hash = |m: &SkipMap<u32, u32>| {
    let mut h = DefaultHasher::new();
    m.hash(&mut h);
    h.finish()
  };
  assert_eq!(hash(&a), hash(&b));
  let mut c = b.clone();
  c.insert(3, 100);
  assert_ne!(a, c);
  assert!(a < c);
  let oracle: BTreeMap<u32, u32> = (0..10).map(|k| (k, k)).collect();
  assert_eq!(format!("{a:?}"), format!("{oracle:?}"));
  assert_eq!(SkipMap::<u32, u32>::default().len(), 0);
}

#[test]
fn empty_map_edges() {
  let mut map: SkipMap<u32, u32> = SkipMap::new();
  assert_eq!(map.pop_first(), None);
  assert_eq!(map.pop_last(), None);
  assert_eq!(map.first_key_value(), None);
  assert_eq!(map.remove(&1), None);
  assert_eq!(map.iter().next(), None);
  assert_eq!(map.range(..).next_back(), None);
  map.clear();
  map.check_invariants();
}

#[test]
fn over_aligned_and_zero_sized_types() {
  #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
  #[repr(align(64))]
  struct Wide(u64);
  let mut map = SkipMap::new();
  for k in 0..500 {
    map.insert(Wide(k), ());
  }
  map.check_invariants();
  for (k, _) in map.iter() {
    assert_eq!(k as *const Wide as usize % 64, 0);
  }
  let mut unit = SkipMap::new();
  assert_eq!(unit.insert((), 1u8), None);
  assert_eq!(unit.insert((), 2u8), Some(1));
  assert_eq!(unit.len(), 1);
  unit.check_invariants();
}

#[test]
fn large_sequential_and_reverse_inserts() {
  let mut map = SkipMap::new();
  let n = scaled(20_000u32);
  for k in 0..n {
    map.insert(k, k);
  }
  for k in (n..2 * n).rev() {
    map.insert(k, k);
  }
  map.check_invariants();
  assert!(map.keys().copied().eq(0..2 * n));
  for k in (0..2 * n).step_by(2) {
    assert_eq!(map.remove(&k), Some(k));
  }
  map.check_invariants();
  assert_eq!(map.len(), n as usize);
}

#[test]
fn thread_traits() {
  fn send_sync<T: Send + Sync>() {}
  send_sync::<SkipMap<u32, String>>();
  send_sync::<super::Iter<'static, u32, String>>();
  send_sync::<super::IntoIter<u32, String>>();
  fn covariant<'a>(m: SkipMap<&'static str, &'static str>) -> SkipMap<&'a str, &'a str> {
    m
  }
  let _ = covariant(SkipMap::new());
}

#[test]
fn entry_matches_btreemap() {
  use super::Entry;
  use std::collections::btree_map;
  let mut rng = Rng(0xE7);
  let mut map: SkipMap<u64, u64> = SkipMap::new();
  let mut oracle: BTreeMap<u64, u64> = BTreeMap::new();
  for step in 0..scaled(5000u64) {
    let key = rng.below(200);
    match rng.below(5) {
      0 => {
        *map.entry(key).or_insert(step) += 1;
        *oracle.entry(key).or_insert(step) += 1;
      }
      1 => {
        map.entry(key).and_modify(|v| *v = 0).or_default();
        oracle.entry(key).and_modify(|v| *v = 0).or_default();
      }
      2 => match (map.entry(key), oracle.entry(key)) {
        (Entry::Occupied(a), btree_map::Entry::Occupied(b)) => {
          assert_eq!(a.key(), b.key());
          assert_eq!(a.remove_entry(), b.remove_entry());
        }
        (Entry::Vacant(a), btree_map::Entry::Vacant(b)) => {
          assert_eq!(a.key(), b.key());
          assert_eq!(*a.insert(step), *b.insert(step));
        }
        _ => panic!("entry kinds differ for {key}"),
      },
      3 => {
        if let (Entry::Occupied(mut a), btree_map::Entry::Occupied(mut b)) = (map.entry(key), oracle.entry(key)) {
          assert_eq!(a.insert(step), b.insert(step));
          *a.get_mut() += 1;
          *b.get_mut() += 1;
          assert_eq!(*a.into_mut(), *b.into_mut());
        }
      }
      _ => assert_eq!(map.entry(key).key(), &key),
    }
    if step % 89 == 0 {
      assert_same(&map, &oracle);
    }
  }
  assert_same(&map, &oracle);
}

#[test]
fn split_off_and_append_match_btreemap() {
  let mut rng = Rng(0x5A);
  let mut map: SkipMap<u64, u64> = SkipMap::new();
  let mut oracle: BTreeMap<u64, u64> = BTreeMap::new();
  for step in 0..scaled(3000u64) {
    let key = rng.below(1000);
    match rng.below(4) {
      0 | 1 => assert_eq!(map.insert(key, step), oracle.insert(key, step)),
      2 => {
        let mut tail = map.split_off(&key);
        let mut oracle_tail = oracle.split_off(&key);
        assert_same(&tail, &oracle_tail);
        assert_same(&map, &oracle);
        if rng.below(3) == 0 {
          let overlapping = rng.below(1000);
          tail.insert(overlapping, step);
          oracle_tail.insert(overlapping, step);
        }
        map.append(&mut tail);
        oracle.append(&mut oracle_tail);
        assert!(tail.is_empty());
        tail.check_invariants();
      }
      _ => assert_eq!(map.remove(&key), oracle.remove(&key)),
    }
    if step % 53 == 0 {
      assert_same(&map, &oracle);
    }
  }
  assert_same(&map, &oracle);
}

#[test]
fn split_halves_outlive_each_other() {
  let mut map: SkipMap<u64, String> = (0..3000).map(|k| (k, k.to_string())).collect();
  let mut tail = map.split_off(&1000);
  drop(map);
  for k in 3000..4000 {
    tail.insert(k, k.to_string());
  }
  let rest = tail.split_off(&2000);
  drop(tail);
  rest.check_invariants();
  assert!(rest.keys().copied().eq(2000..4000));
}
