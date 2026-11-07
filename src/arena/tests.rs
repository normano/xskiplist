use super::ArenaSkipMap;
use crate::test_util::{scaled, scaled_mt};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
}

fn assert_same(map: &ArenaSkipMap<u64, u64>, oracle: &BTreeMap<u64, u64>) {
  map.check_invariants();
  assert_eq!(map.len(), oracle.len());
  assert!(map.iter().eq(oracle.iter()));
  assert_eq!(map.first(), oracle.first_key_value());
  assert_eq!(map.last(), oracle.last_key_value());
}

#[test]
fn single_thread_matches_btreemap() {
  for seed in 1..=4u64 {
    let mut rng = Rng(seed);
    let map = ArenaSkipMap::new();
    let mut oracle = BTreeMap::new();
    for _ in 0..scaled(4000u64) {
      let k = rng.next() % 2048;
      let v = rng.next();
      let (_, stored, inserted) = map.insert(k, v);
      let was_absent = oracle.insert(k, v).is_none();
      assert_eq!(inserted, was_absent);
      if !inserted {
        oracle.insert(k, *stored);
      }
      assert_eq!(map.get(&k), oracle.get(&k));
    }
    assert_same(&map, &oracle);
    for _ in 0..scaled(200u64) {
      let a = rng.next() % 2200;
      let b = rng.next() % 2200;
      let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
      assert!(map.range(lo..hi).eq(oracle.range(lo..hi)), "range {lo}..{hi}");
      assert!(map.range(lo..=hi).eq(oracle.range(lo..=hi)), "range {lo}..={hi}");
      assert!(map.range(..hi).eq(oracle.range(..hi)));
      assert!(map.range(lo..).eq(oracle.range(lo..)));
    }
    assert!(map.range(..).eq(oracle.range(..)));
    assert!(map.range(5000..).eq(oracle.range(5000..)));
  }
}

#[test]
fn get_or_insert_keeps_first_value() {
  let map = ArenaSkipMap::new();
  assert_eq!(*map.get_or_insert(1, "a"), "a");
  assert_eq!(*map.get_or_insert(1, "b"), "a");
  assert_eq!(*map.get_or_insert_with(2, || "c"), "c");
  assert_eq!(*map.get_or_insert_with(2, || panic!("absent only")), "c");
  assert_eq!(map.len(), 2);
  assert!(map.contains_key(&1));
  assert!(!map.contains_key(&3));
  assert_eq!(map.get_key_value(&2), Some((&2, &"c")));
}

#[test]
fn concurrent_inserts_land_once() {
  let threads = 8u64;
  let per = scaled_mt(20_000u64);
  let map = Arc::new(ArenaSkipMap::<u64, u64>::new());
  let wins = Arc::new(AtomicUsize::new(0));
  let handles: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      let wins = Arc::clone(&wins);
      thread::spawn(move || {
        let mut rng = Rng(t + 1);
        let mut local = 0;
        for i in 0..per {
          let own = i * threads + t;
          if map.insert(own, t).2 {
            local += 1;
          }
          let shared = rng.next() % 1024;
          if map.insert(u64::MAX - shared, t).2 {
            local += 1;
          }
        }
        wins.fetch_add(local, Ordering::Relaxed);
      })
    })
    .collect();
  for h in handles {
    h.join().unwrap();
  }
  map.check_invariants();
  assert_eq!(map.len(), wins.load(Ordering::Relaxed));
  for i in 0..per * threads {
    assert_eq!(map.get(&i), Some(&(i % threads)), "key {i}");
  }
  let shared: Vec<_> = map.range(u64::MAX - 1024..).collect();
  assert!(shared.len() <= 1024);
  assert!(shared.windows(2).all(|w| w[0].0 < w[1].0));
  assert_eq!(map.len(), per as usize * threads as usize + shared.len());
}

#[test]
fn readers_see_sorted_prefixes_during_inserts() {
  let map = Arc::new(ArenaSkipMap::<u64, u64>::new());
  let n = scaled_mt(50_000u64);
  let writer = {
    let map = Arc::clone(&map);
    thread::spawn(move || {
      let mut rng = Rng(7);
      for _ in 0..n {
        let k = rng.next() % (4 * n);
        map.insert(k, k * 2);
      }
    })
  };
  let readers: Vec<_> = (0..3)
    .map(|_| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        for _ in 0..scaled(40u64) {
          let mut prev = None;
          for (k, v) in map.iter() {
            assert_eq!(*v, *k * 2);
            if let Some(p) = prev {
              assert!(p < *k);
            }
            prev = Some(*k);
          }
        }
      })
    })
    .collect();
  writer.join().unwrap();
  for r in readers {
    r.join().unwrap();
  }
  map.check_invariants();
}

#[test]
fn every_value_dropped_once() {
  struct Counted(Arc<AtomicUsize>);
  impl Drop for Counted {
    fn drop(&mut self) {
      self.0.fetch_add(1, Ordering::Relaxed);
    }
  }
  let drops = Arc::new(AtomicUsize::new(0));
  let n = scaled(2000usize);
  let keys = n.min(500);
  {
    let map = ArenaSkipMap::new();
    for i in 0..n {
      map.insert(i % keys, Counted(Arc::clone(&drops)));
    }
    assert_eq!(map.len(), keys);
    assert_eq!(drops.load(Ordering::Relaxed), n - keys);
  }
  assert_eq!(drops.load(Ordering::Relaxed), n);
}

#[test]
fn string_keys_borrow() {
  let map: ArenaSkipMap<String, usize> = [("b".to_string(), 2), ("a".to_string(), 1)].into_iter().collect();
  assert_eq!(map.get("a"), Some(&1));
  assert_eq!(map.get("c"), None);
  let keys: Vec<&str> = map.iter().map(|(k, _)| k.as_str()).collect();
  assert_eq!(keys, ["a", "b"]);
}
